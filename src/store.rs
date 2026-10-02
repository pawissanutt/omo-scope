use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::Timestamp;
use serde_json::Value;

use crate::text::{clip, sanitize};

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub status: String,
    pub label: String,
    pub category: String,
    pub model: String,
    pub parent_session: String,
    pub root_session: String,
    pub created: Option<Timestamp>,
    pub started: Option<Timestamp>,
    pub finished: Option<Timestamp>,
    pub updated: Option<Timestamp>,
    pub turns: Option<u64>,
    pub tool_calls: Option<u64>,
    pub tokens: Option<u64>,
    pub error: Option<String>,
    pub session_path: Option<PathBuf>,
}

fn ts(v: &Value) -> Option<Timestamp> {
    v.as_str().and_then(|s| s.parse().ok())
}

fn text_of(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(sanitize)
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Task {
    pub fn from_json(v: &Value) -> Option<Self> {
        let id = v["task_id"].as_str().filter(|id| valid_id(id))?.to_string();
        let label = text_of(&v["task_summary"])
            .or_else(|| text_of(&v["description"]))
            .or_else(|| text_of(&v["name"]))
            .unwrap_or_else(|| id.clone());
        let model = text_of(&v["resolved_model"]["model_id"])
            .or_else(|| text_of(&v["model"]).map(|m| m.rsplit('/').next().unwrap_or(&m).to_string()))
            .unwrap_or_default();
        let stats = &v["run_stats"];
        Some(Self {
            status: text_of(&v["status"]).unwrap_or_else(|| "unknown".into()),
            label: clip(crate::text::first_line(&label), 300),
            category: text_of(&v["category"])
                .or_else(|| text_of(&v["agent_type"]))
                .unwrap_or_default(),
            model,
            parent_session: v["parent_session_id"].as_str().unwrap_or("").to_string(),
            root_session: v["root_session_id"].as_str().unwrap_or("").to_string(),
            created: ts(&v["created_at"]),
            started: ts(&v["started_at"]),
            finished: ts(&v["terminal_at"]),
            updated: ts(&v["updated_at"]),
            turns: stats["turns"].as_u64(),
            tool_calls: stats["tool_calls"].as_u64(),
            tokens: stats["total_tokens"].as_u64(),
            error: text_of(&v["error_message"]),
            session_path: v["host_session"]["session_path"].as_str().map(PathBuf::from),
            id,
        })
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            "completed" | "error" | "failed" | "cancelled" | "interrupted" | "lost"
        )
    }

    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }

    pub fn elapsed_secs(&self) -> Option<i64> {
        let start = self.started.or(self.created)?;
        let end = match self.finished {
            Some(f) => f,
            None if self.is_terminal() => self.updated.unwrap_or(start),
            None => Timestamp::now(),
        };
        Some(end.as_second() - start.as_second())
    }
}

#[derive(Debug, Clone)]
pub struct DagNode {
    pub id: String,
    pub label: String,
    pub state: String,
    pub depends_on: Vec<String>,
    pub task_id: Option<String>,
    pub level: usize,
}

#[derive(Debug, Clone)]
pub struct DagRun {
    pub id: String,
    pub name: String,
    pub status: String,
    pub parent_session: String,
    pub root_session: String,
    pub updated: Option<Timestamp>,
    pub nodes: Vec<DagNode>,
}

impl DagRun {
    pub fn from_json(v: &Value) -> Option<Self> {
        let id = v["runId"].as_str()?.to_string();
        let summaries: HashMap<&str, String> = v["definition"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| Some((n["id"].as_str()?, text_of(&n["task_summary"])?)))
            .collect();
        let mut nodes: Vec<DagNode> = v["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| {
                let nid = n["id"].as_str()?.to_string();
                let label = summaries
                    .get(nid.as_str())
                    .cloned()
                    .or_else(|| text_of(&n["task_summary"]))
                    .unwrap_or_else(|| sanitize(&nid));
                Some(DagNode {
                    label: clip(crate::text::first_line(&label), 300),
                    state: text_of(&n["state"]).unwrap_or_else(|| "pending".into()),
                    depends_on: n["dependsOn"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|d| d.as_str().map(str::to_string))
                        .collect(),
                    task_id: n["taskId"].as_str().filter(|t| valid_id(t)).map(str::to_string),
                    level: 0,
                    id: nid,
                })
            })
            .collect();
        let index: HashMap<String, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        for _ in 0..nodes.len() {
            let mut changed = false;
            for i in 0..nodes.len() {
                let level = nodes[i]
                    .depends_on
                    .iter()
                    .filter_map(|d| index.get(d))
                    .map(|&j| nodes[j].level + 1)
                    .max()
                    .unwrap_or(0);
                if level != nodes[i].level {
                    nodes[i].level = level;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        nodes.sort_by_key(|n| n.level);
        Some(Self {
            name: text_of(&v["name"]).unwrap_or_else(|| id.clone()),
            status: text_of(&v["status"]).unwrap_or_else(|| "unknown".into()),
            parent_session: v["parentSessionId"].as_str().unwrap_or("").to_string(),
            root_session: v["rootSessionId"].as_str().unwrap_or("").to_string(),
            updated: ts(&v["updatedAt"]).or_else(|| ts(&v["createdAt"])),
            nodes,
            id,
        })
    }

    pub fn belongs_to(&self, session: &str) -> bool {
        self.root_session == session || self.parent_session == session
    }

    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .nodes
            .iter()
            .filter(|n| matches!(n.state.as_str(), "completed" | "succeeded" | "done" | "skipped"))
            .count();
        (done, self.nodes.len())
    }
}

type Stamp = (SystemTime, u64);

struct Cached<T> {
    stamp: Stamp,
    value: Option<T>,
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()))
}

fn refresh_dir<T>(dir: &Path, cache: &mut HashMap<PathBuf, Cached<T>>, parse: impl Fn(&Value) -> Option<T>) -> bool {
    let mut changed = false;
    let mut seen = HashSet::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(stamp) = stamp_of(&path) else { continue };
        seen.insert(path.clone());
        if cache.get(&path).is_some_and(|c| c.stamp == stamp) {
            continue;
        }
        let value = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| parse(&v));
        cache.insert(path, Cached { stamp, value });
        changed = true;
    }
    let before = cache.len();
    cache.retain(|p, _| seen.contains(p));
    changed || cache.len() != before
}

pub struct Store {
    dir: PathBuf,
    tasks: HashMap<PathBuf, Cached<Task>>,
    runs: HashMap<PathBuf, Cached<DagRun>>,
    child_sessions: HashMap<String, String>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            tasks: HashMap::new(),
            runs: HashMap::new(),
            child_sessions: HashMap::new(),
        }
    }

    pub fn refresh(&mut self) -> bool {
        let a = refresh_dir(&self.dir.join("tasks"), &mut self.tasks, Task::from_json);
        let b = refresh_dir(&self.dir.join("dag").join("runs"), &mut self.runs, DagRun::from_json);
        let mut changed = a || b;
        let missing: Vec<(String, PathBuf)> = self
            .tasks()
            .filter(|t| !self.child_sessions.contains_key(&t.id))
            .filter_map(|t| Some((t.id.clone(), self.transcript_path(&t.id)?)))
            .collect();
        for (id, path) in missing {
            if let Some(session) = session_header_id(&path) {
                self.child_sessions.insert(id, session);
                changed = true;
            }
        }
        changed
    }

    pub fn invalid(&self) -> usize {
        let bad_tasks = self.tasks.values().filter(|c| c.value.is_none()).count();
        bad_tasks + self.runs.values().filter(|c| c.value.is_none()).count()
    }

    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values().filter_map(|c| c.value.as_ref())
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks().find(|t| t.id == id)
    }

    pub fn run(&self, id: &str) -> Option<&DagRun> {
        self.runs.values().filter_map(|c| c.value.as_ref()).find(|r| r.id == id)
    }

    pub fn runs_for(&self, session: &str) -> Vec<&DagRun> {
        let mut runs: Vec<&DagRun> = self
            .runs
            .values()
            .filter_map(|c| c.value.as_ref())
            .filter(|r| r.belongs_to(session))
            .collect();
        runs.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        runs
    }

    pub fn transcript_path(&self, task_id: &str) -> Option<PathBuf> {
        if !valid_id(task_id) {
            return None;
        }
        let dir = self.dir.join("children").join(task_id).join("sessions").join(task_id);
        newest_jsonl(&dir).or_else(|| {
            let t = self.task(task_id)?;
            t.session_path.clone().filter(|p| p.is_file())
        })
    }

    pub fn session_tree(&self, session: &str) -> Vec<(usize, &Task)> {
        let mut by_parent: HashMap<&str, Vec<&Task>> = HashMap::new();
        let members: Vec<&Task> = self
            .tasks()
            .filter(|t| t.root_session == session || t.parent_session == session)
            .collect();
        for t in &members {
            by_parent.entry(t.parent_session.as_str()).or_default().push(t);
        }
        for list in by_parent.values_mut() {
            list.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.walk(session, 0, &by_parent, &mut seen, &mut out);
        let mut orphans: Vec<&Task> = members.into_iter().filter(|t| !seen.contains(t.id.as_str())).collect();
        orphans.sort_by_key(|t| t.created);
        out.extend(orphans.into_iter().map(|t| (0, t)));
        out
    }

    fn walk<'a>(
        &self,
        session: &str,
        depth: usize,
        by_parent: &HashMap<&str, Vec<&'a Task>>,
        seen: &mut HashSet<&'a str>,
        out: &mut Vec<(usize, &'a Task)>,
    ) {
        for &t in by_parent.get(session).into_iter().flatten() {
            if !seen.insert(t.id.as_str()) {
                continue;
            }
            out.push((depth, t));
            if let Some(child) = self.child_sessions.get(&t.id) {
                self.walk(child, depth + 1, by_parent, seen, out);
            }
        }
    }

    fn root_counts(&self) -> HashMap<String, (usize, usize, Option<Timestamp>)> {
        let mut counts: HashMap<String, (usize, usize, Option<Timestamp>)> = HashMap::new();
        for t in self.tasks() {
            let root = if t.root_session.is_empty() {
                &t.parent_session
            } else {
                &t.root_session
            };
            let c = counts.entry(root.clone()).or_default();
            c.0 += 1;
            c.1 += usize::from(t.is_active());
            c.2 = c.2.max(t.updated.or(t.created));
        }
        counts
    }
}

fn newest_jsonl(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .max_by(|a, b| a.file_name().cmp(&b.file_name()))
}

fn session_header_id(path: &Path) -> Option<String> {
    let mut line = String::new();
    BufReader::new(fs::File::open(path).ok()?.take(64 * 1024))
        .read_line(&mut line)
        .ok()?;
    let v: Value = serde_json::from_str(&line).ok()?;
    if v["type"] != "session" {
        return None;
    }
    v["id"].as_str().map(str::to_string)
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub modified: Option<Timestamp>,
    pub tasks: usize,
    pub active: usize,
}

pub struct SessionIndex {
    dir: PathBuf,
    titles: HashMap<PathBuf, (Stamp, String)>,
}

impl SessionIndex {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            titles: HashMap::new(),
        }
    }

    pub fn list(&mut self, store: &Store) -> Vec<SessionInfo> {
        let mut counts = store.root_counts();
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some((_, id)) = stem.split_once('_') else { continue };
            let Some(stamp) = stamp_of(&path) else { continue };
            let title = match self.titles.get(&path) {
                Some((s, title)) if *s == stamp => title.clone(),
                _ => {
                    let title = read_title(&path);
                    self.titles.insert(path.clone(), (stamp, title.clone()));
                    title
                }
            };
            let (tasks, active, _) = counts.remove(id).unwrap_or_default();
            let modified = Timestamp::try_from(stamp.0).ok();
            out.push(SessionInfo {
                id: id.to_string(),
                title,
                modified,
                tasks,
                active,
            });
        }
        for (id, (tasks, active, modified)) in counts {
            if !id.is_empty() {
                out.push(SessionInfo {
                    id,
                    title: String::new(),
                    modified,
                    tasks,
                    active,
                });
            }
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.id.cmp(&b.id)));
        out
    }
}

fn read_title(path: &Path) -> String {
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    let mut name = None;
    let mut first_user = None;
    for line in BufReader::new(file.take(512 * 1024)).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("session_info") => name = text_of(&v["name"]).or(name),
            Some("message") if first_user.is_none() && v["message"]["role"] == "user" => {
                first_user = Some(user_title(&v["message"]["content"])).filter(|s| !s.is_empty());
            }
            _ => {}
        }
    }
    clip(&name.or(first_user).unwrap_or_default(), 160)
}

fn user_title(content: &Value) -> String {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let mut in_tag = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with("</") {
            in_tag = false;
        } else if line.starts_with('<') && line.ends_with('>') {
            in_tag = !line.contains("</");
        } else if !in_tag && !line.is_empty() {
            return sanitize(line);
        }
    }
    String::new()
}

pub fn sessions_dir(project: &Path) -> PathBuf {
    let agent = ["OMO_CODING_AGENT_DIR", "SENPI_CODING_AGENT_DIR"]
        .iter()
        .find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from))
        .unwrap_or_else(|| home().join(".omo").join("agent"));
    agent.join("sessions").join(encode_cwd(project))
}

pub fn encode_cwd(project: &Path) -> String {
    let s = project.to_string_lossy();
    let body: String = s
        .trim_start_matches(['/', '\\'])
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c })
        .collect();
    format!("--{body}--")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn project_root(start: &Path) -> PathBuf {
    start
        .ancestors()
        .find(|p| p.join(".omo").join("senpi-task").is_dir())
        .unwrap_or(start)
        .to_path_buf()
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
