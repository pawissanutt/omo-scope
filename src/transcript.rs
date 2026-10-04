use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde_json::Value;

use crate::diff::{self, DiffLine};
use crate::pinned::{Goal, Plan};
use crate::stats::Usage;
use crate::text::{clip, first_line, sanitize};

pub struct Tail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

#[derive(Default)]
pub struct Batch {
    pub values: Vec<Value>,
    pub bad: usize,
    pub reset: bool,
}

impl Tail {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: Vec::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn poll(&mut self) -> std::io::Result<Batch> {
        let mut file = File::open(&self.path)?;
        let len = file.metadata()?.len();
        let mut batch = Batch::default();
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
            batch.reset = true;
        }
        if len == self.offset {
            return Ok(batch);
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::new();
        file.by_ref().take(len - self.offset).read_to_end(&mut buf)?;
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);
        let Some(end) = self.partial.iter().rposition(|&b| b == b'\n') else {
            return Ok(batch);
        };
        let complete: Vec<u8> = self.partial.drain(..=end).collect();
        for line in complete.split(|&b| b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice(line) {
                Ok(v) => batch.values.push(v),
                Err(_) => batch.bad += 1,
            }
        }
        Ok(batch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    User,
    Assistant,
    Thinking,
    Tool,
    Note,
    Error,
    Summary,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

/// A subagent task referenced by a tool call; rendered as a clickable line with its live status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub task: String,
    pub name: String,
    pub note: String,
    pub error: bool,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: Kind,
    pub title: String,
    pub text: String,
    pub detail: String,
    pub result: Option<ToolResult>,
    pub at: Option<Timestamp>,
    pub command: String,
    pub diff: Vec<DiffLine>,
    /// Replaces the collapsed result preview; empty hides it. Errors always show the result.
    pub preview: Option<String>,
    pub links: Vec<Link>,
}

impl Entry {
    pub(crate) fn new(kind: Kind, title: impl Into<String>, text: impl Into<String>, at: Option<Timestamp>) -> Self {
        Self {
            kind,
            title: title.into(),
            text: text.into(),
            detail: String::new(),
            result: None,
            at,
            command: String::new(),
            diff: Vec::new(),
            preview: None,
            links: Vec::new(),
        }
    }

    pub fn collapsible(&self) -> bool {
        matches!(self.kind, Kind::User | Kind::Thinking | Kind::Tool | Kind::Summary)
    }
}

#[derive(Default)]
pub struct Transcript {
    pub entries: Vec<Entry>,
    pub last_at: Option<Timestamp>,
    pub cwd: Option<PathBuf>,
    pub usage: Usage,
    pub plan: Option<Plan>,
    pub goal: Option<Goal>,
    names: HashMap<String, String>,
    open: HashMap<String, (usize, Value)>,
}

impl Transcript {
    pub fn push(&mut self, v: &Value) {
        self.usage.push(v);
        let at = v["timestamp"].as_str().and_then(|s| s.parse::<Timestamp>().ok());
        if at.is_some() {
            self.last_at = at;
        }
        match v["type"].as_str().unwrap_or("") {
            "session" => self.cwd = v["cwd"].as_str().map(PathBuf::from),
            "message" => self.push_message(&v["message"], at),
            "model_change" => {
                let model = format!("{}/{}", str_of(&v["provider"]), str_of(&v["modelId"]));
                self.entries.push(Entry::new(Kind::Note, "model", model, at));
            }
            "compaction" => self.push_summary("compacted", &v["summary"], at),
            "branch_summary" => self.push_summary("branch summary", &v["summary"], at),
            "custom" if v["customType"].as_str() == Some("senpi.todo-state") => {
                self.plan = Plan::from_state(&v["data"]);
            }
            "custom_message" => {
                if v["customType"].as_str() == Some("goal-continuation")
                    && let Some(g) = self.goal.as_mut()
                {
                    g.wakes += 1;
                }
                if v["display"].as_bool() == Some(true) {
                    let text = content_text(&v["content"]);
                    self.entries
                        .push(Entry::new(Kind::Note, sanitize(str_of(&v["customType"])), text, at));
                }
            }
            _ => {}
        }
    }

    pub fn running_index(&self) -> Option<usize> {
        self.open.values().map(|(i, _)| *i).max()
    }

    pub fn running_tool(&self) -> Option<&Entry> {
        self.running_index().map(|i| &self.entries[i])
    }

    fn push_summary(&mut self, title: &str, summary: &Value, at: Option<Timestamp>) {
        self.entries
            .push(Entry::new(Kind::Summary, title, sanitize(str_of(summary)), at));
    }

    fn push_message(&mut self, m: &Value, at: Option<Timestamp>) {
        match m["role"].as_str().unwrap_or("") {
            "user" => {
                let text = content_text(&m["content"]);
                self.entries.push(Entry::new(Kind::User, "user", text, at));
            }
            "assistant" => self.push_assistant(m, at),
            "toolResult" => {
                let result = ToolResult {
                    text: content_text(&m["content"]),
                    is_error: m["isError"].as_bool().unwrap_or(false),
                };
                if !result.is_error && str_of(&m["toolName"]).ends_with("_goal") {
                    self.set_goal(&m["details"], &result.text);
                }
                let (i, args) = match self.open.remove(str_of(&m["toolCallId"])) {
                    Some(open) => open,
                    None => {
                        let name = sanitize(str_of(&m["toolName"]));
                        self.entries.push(Entry::new(Kind::Tool, name, "", at));
                        (self.entries.len() - 1, Value::Null)
                    }
                };
                let e = &mut self.entries[i];
                e.result = Some(result);
                crate::tools::result(e, &args, m, &mut self.names);
            }
            "bashExecution" => {
                let mut e = Entry::new(Kind::Tool, "bash", sanitize(str_of(&m["command"])), at);
                e.result = Some(ToolResult {
                    text: sanitize(str_of(&m["output"])),
                    is_error: m["exitCode"].as_i64().is_some_and(|c| c != 0),
                });
                self.entries.push(e);
            }
            "compactionSummary" => self.push_summary("compacted", &m["summary"], at),
            "branchSummary" => self.push_summary("branch summary", &m["summary"], at),
            "custom" if m["display"].as_bool() == Some(true) => {
                let text = content_text(&m["content"]);
                self.entries
                    .push(Entry::new(Kind::Note, sanitize(str_of(&m["customType"])), text, at));
            }
            _ => {}
        }
    }

    fn push_assistant(&mut self, m: &Value, at: Option<Timestamp>) {
        // The agent loop only starts a new assistant turn after every prior tool call
        // has returned, so calls still open here were abandoned (abort, crash, reload).
        for (_, (i, _)) in self.open.drain() {
            let text = "(no result recorded)".to_string();
            self.entries[i].result = Some(ToolResult { text, is_error: true });
        }
        for block in m["content"].as_array().into_iter().flatten() {
            match block["type"].as_str().unwrap_or("") {
                "text" => self.push_text(Kind::Assistant, "", str_of(&block["text"]), at),
                "thinking" => self.push_text(Kind::Thinking, "thinking", str_of(&block["thinking"]), at),
                "toolCall" => {
                    let args = &block["arguments"];
                    let name = sanitize(str_of(&block["name"]));
                    let change = diff::from_call(&name, args, self.cwd.as_deref());
                    let mut e = Entry::new(Kind::Tool, name, summarize_args(args), at);
                    if let Some(c) = change {
                        e.text = c.summary;
                        e.diff = c.lines;
                    }
                    e.detail = sanitize(&serde_json::to_string_pretty(args).unwrap_or_default());
                    e.command = match args["command"].as_str() {
                        Some(c) => sanitize(c),
                        None => sanitize(&embedded_commands(args["code"].as_str().unwrap_or(""))),
                    };
                    crate::tools::call(&mut e, args, self.cwd.as_deref());
                    let open = (self.entries.len(), args.clone());
                    self.open.insert(str_of(&block["id"]).to_string(), open);
                    self.entries.push(e);
                }
                _ => {}
            }
        }
        match m["stopReason"].as_str() {
            Some("error") => {
                let msg = m["errorMessage"].as_str().unwrap_or("model error");
                self.entries.push(Entry::new(Kind::Error, "error", sanitize(msg), at));
            }
            Some("aborted") => self.entries.push(Entry::new(Kind::Error, "aborted", "", at)),
            _ => {}
        }
    }

    /// `details.goal` of a goal-tool result (or the JSON text when details are missing); null clears it.
    fn set_goal(&mut self, details: &Value, text: &str) {
        let parsed;
        let root = if details.get("goal").is_some() {
            details
        } else {
            parsed = serde_json::from_str::<Value>(text).unwrap_or_default();
            &parsed
        };
        match root.get("goal") {
            Some(Value::Null) => self.goal = None,
            Some(g) => {
                if let Some(mut goal) = Goal::from_json(g) {
                    if let Some(old) = self.goal.as_ref().filter(|o| o.objective == goal.objective) {
                        goal.wakes = old.wakes;
                    }
                    self.goal = Some(goal);
                }
            }
            None => {}
        }
    }

    fn push_text(&mut self, kind: Kind, title: &str, raw: &str, at: Option<Timestamp>) {
        let text = sanitize(raw);
        if !text.trim().is_empty() {
            self.entries.push(Entry::new(kind, title, text.trim(), at));
        }
    }
}

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn content_text(v: &Value) -> String {
    match v {
        Value::String(s) => sanitize(s),
        Value::Array(blocks) => {
            let parts: Vec<String> = blocks
                .iter()
                .filter_map(|b| match b["type"].as_str() {
                    Some("text") => Some(sanitize(str_of(&b["text"]))),
                    Some("image") => Some("[image]".to_string()),
                    _ => None,
                })
                .collect();
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn embedded_commands(code: &str) -> String {
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("command") {
        rest = &rest[at + "command".len()..];
        let Some(after) = rest.trim_start_matches(['"', '\'', ' ']).strip_prefix(':') else {
            continue;
        };
        let after = after.trim_start();
        let Some(q) = after.chars().next().filter(|c| matches!(c, '"' | '\'' | '`')) else {
            continue;
        };
        let mut s = String::new();
        let mut escaped = false;
        for c in after[1..].chars() {
            if escaped {
                s.push(if c == 'n' { '\n' } else { c });
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                out.push(std::mem::take(&mut s));
                break;
            } else {
                s.push(c);
            }
        }
    }
    out.join("\n")
}

const ARG_KEYS: &[&str] = &[
    "command",
    "cmd",
    "path",
    "file_path",
    "filePath",
    "pattern",
    "query",
    "url",
    "description",
    "summary",
    "task_summary",
    "prompt",
    "code",
];

pub(crate) fn summarize_args(args: &Value) -> String {
    for key in ARG_KEYS {
        if let Some(s) = args[*key].as_str() {
            return clip(&sanitize(first_line(s)), 200);
        }
    }
    match args {
        Value::Null => String::new(),
        Value::Object(o) if o.is_empty() => String::new(),
        other => clip(&sanitize(&other.to_string()), 200),
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
