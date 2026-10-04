//! Tool-specific call text, result previews and subagent links for the transcript log.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::pinned::Goal;
use crate::store::valid_id;
use crate::text::{first_line, fmt_duration, sanitize};
use crate::transcript::{Entry, Link, summarize_args};

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn one(v: &Value) -> String {
    sanitize(first_line(s(v)))
}

fn target(args: &Value) -> String {
    ["to", "name", "task_id"]
        .iter()
        .map(|k| one(&args[*k]))
        .find(|t| !t.is_empty())
        .unwrap_or_default()
}

pub fn call(e: &mut Entry, args: &Value, cwd: Option<&Path>) {
    let text = match e.title.as_str() {
        "todo" => todo_text(args),
        "create_goal" => format!("◎ {}", one(&args["objective"])),
        "update_goal" => match one(&args["reason"]) {
            r if r.is_empty() => format!("→ {}", one(&args["status"])),
            r => format!("→ {}: {r}", one(&args["status"])),
        },
        "get_goal" => String::new(),
        "task" => spawn_text(args),
        "task_output" => match one(&args["mode"]) {
            m if m.is_empty() => format!("peek {}", target(args)),
            m => format!("peek {} ({m})", target(args)),
        },
        "task_send" => format!("→ {}: {}", target(args), one(&args["message"])),
        "task_cancel" => format!("cancel {}: {}", target(args), one(&args["reason"])),
        "eval" => eval_text(args),
        "read" => match read_text(args, cwd) {
            Some(t) => t,
            None => return,
        },
        "memory" => memory_text(args),
        _ => return,
    };
    e.text = text.trim_end_matches([' ', ':']).to_string();
    if e.title == "eval"
        && let Some(code) = args["code"].as_str()
    {
        e.detail = sanitize(code);
    }
}

fn todo_text(a: &Value) -> String {
    let what = [&a["task"], &a["phase"]]
        .into_iter()
        .map(one)
        .find(|w| !w.is_empty())
        .unwrap_or_default();
    let items = a["items"].as_array().map_or(0, Vec::len);
    match s(&a["op"]) {
        "init" => {
            let (phases, n) = match a["list"].as_array() {
                Some(l) => (
                    l.len(),
                    l.iter().map(|p| p["items"].as_array().map_or(0, Vec::len)).sum(),
                ),
                None => (1, items),
            };
            format!("new plan · {n} tasks in {phases} phases")
        }
        op @ ("start" | "done" | "drop") => format!("{op} · {what}"),
        "append" => match one(&a["phase"]) {
            p if p.is_empty() => format!("+ {items} tasks"),
            p => format!("+ {items} tasks → {p}"),
        },
        "rm" if what.is_empty() => "clear plan".into(),
        "rm" => format!("remove {what}"),
        "view" => "view plan".into(),
        other => sanitize(other),
    }
}

fn spawn_label(t: &Value) -> String {
    let name = one(&t["name"]);
    let summary = [&t["task_summary"], &t["description"]]
        .into_iter()
        .map(one)
        .find(|x| !x.is_empty())
        .unwrap_or_default();
    match (name.is_empty(), summary.is_empty()) {
        (false, false) => format!("{name} · {summary}"),
        (false, true) => name,
        _ => summary,
    }
}

fn spawn_text(a: &Value) -> String {
    match a["tasks"].as_array() {
        Some(list) => {
            let names: Vec<String> = list
                .iter()
                .map(|t| match one(&t["name"]) {
                    n if n.is_empty() => spawn_label(t),
                    n => n,
                })
                .collect();
            format!("spawn {} · {}", list.len(), names.join(", "))
        }
        None => format!("spawn {}", spawn_label(a)),
    }
}

fn eval_text(a: &Value) -> String {
    let action = one(&a["action"]);
    if !action.is_empty() && action != "run" {
        return format!("{action} {}", one(&a["cell_id"]));
    }
    let summary = match one(&a["summary"]) {
        x if x.is_empty() => one(&a["code"]),
        x => x,
    };
    match one(&a["language"]) {
        l if l.is_empty() => summary,
        l => format!("[{l}] {summary}"),
    }
}

fn read_text(a: &Value, cwd: Option<&Path>) -> Option<String> {
    let path = ["path", "file_path", "filePath"].iter().find_map(|k| a[*k].as_str())?;
    let mut out = crate::diff::short(path, cwd);
    match (a["offset"].as_u64(), a["limit"].as_u64()) {
        (Some(o), Some(l)) => out.push_str(&format!(":{o}-{}", (o + l).saturating_sub(1))),
        (Some(o), None) => out.push_str(&format!(":{o}-")),
        (None, Some(l)) => out.push_str(&format!(":1-{l}")),
        (None, None) => {}
    }
    if let Some(p) = a["pages"].as_str() {
        out.push_str(&format!(" pages {}", sanitize(p)));
    }
    Some(out)
}

fn memory_text(a: &Value) -> String {
    let file = match (one(&a["old_path"]), one(&a["new_path"])) {
        (old, new) if !old.is_empty() && !new.is_empty() => format!("{old} → {new}"),
        _ => one(&a["file_path"]),
    };
    let head = format!("{} {file}", one(&a["command"]));
    match one(&a["reason"]) {
        r if r.is_empty() => head,
        r => format!("{} — {r}", head.trim()),
    }
}

/// Shapes a finished call from its result message. `names` maps subagent names to task ids.
pub fn result(e: &mut Entry, args: &Value, m: &Value, names: &mut HashMap<String, String>) {
    let d = &m["details"];
    let failed = e.result.as_ref().is_some_and(|r| r.is_error);
    match e.title.as_str() {
        "todo" if !failed => e.preview = Some(String::new()),
        "create_goal" | "update_goal" | "get_goal" if !failed => {
            e.preview = match &d["goal"] {
                Value::Null if d.get("goal").is_some() => Some("no goal".into()),
                g => Goal::from_json(g).map(|g| g.summary()),
            };
        }
        "task" => {
            let items = d["items"].as_array().cloned().unwrap_or_else(|| vec![d.clone()]);
            for it in &items {
                let Some(id) = it["task_id"].as_str().filter(|id| valid_id(id)) else {
                    continue;
                };
                let name = one(&it["name"]);
                if !name.is_empty() {
                    names.insert(name.clone(), id.to_string());
                }
                e.links.push(Link {
                    task: id.to_string(),
                    name,
                    note: String::new(),
                    error: false,
                });
            }
            if !e.links.is_empty() {
                e.preview = Some(String::new());
            }
        }
        "task_output" | "task_send" | "task_cancel" => link_target(e, args, d, names, failed),
        "eval" if !failed => eval_result(e, d),
        "read" if !failed => {
            let n = e.result.as_ref().map_or(0, |r| r.text.lines().count());
            e.preview = Some(format!("{n} lines"));
        }
        _ => {}
    }
}

fn link_target(e: &mut Entry, args: &Value, d: &Value, names: &HashMap<String, String>, failed: bool) {
    let tgt = target(args);
    let id = [&d["task_id"], &d["snapshot"]["task_id"]]
        .into_iter()
        .find_map(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| names.get(&tgt).cloned())
        .unwrap_or_else(|| tgt.clone());
    if !valid_id(&id) {
        return;
    }
    let name = if !tgt.is_empty() && tgt != id {
        tgt
    } else {
        names
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    };
    let note = match (e.title.as_str(), &e.result) {
        ("task_output", Some(r)) if !r.is_error => String::new(),
        (_, Some(r)) => first_line(&r.text).to_string(),
        (_, None) => String::new(),
    };
    e.links.push(Link {
        task: id,
        name,
        note,
        error: failed,
    });
    e.preview = Some(String::new());
}

fn fmt_ms(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms}ms"),
        1_000..60_000 => format!("{:.1}s", ms as f64 / 1000.0),
        _ => fmt_duration((ms / 1000) as i64),
    }
}

fn eval_result(e: &mut Entry, d: &Value) {
    let Some(ms) = d["durationMs"].as_u64() else { return };
    let calls = d["toolCalls"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut counts: Vec<(String, usize, bool)> = Vec::new();
    for c in calls {
        let name = one(&c["name"]);
        let failed = c["ok"].as_bool() == Some(false);
        match counts.iter_mut().find(|(n, _, _)| *n == name) {
            Some(entry) => {
                entry.1 += 1;
                entry.2 |= failed;
            }
            None => counts.push((name, 1, failed)),
        }
    }
    let mut preview = fmt_ms(ms);
    for (i, (name, n, failed)) in counts.iter().enumerate() {
        preview.push_str(if i == 0 { " · " } else { ", " });
        preview.push_str(name);
        if *n > 1 {
            preview.push_str(&format!(" ×{n}"));
        }
        if *failed {
            preview.push_str(" ✗");
        }
    }
    let text = e.result.as_ref().map_or("", |r| r.text.as_str());
    if text.contains("detached and is running") {
        preview = format!("⧗ detached · {preview}");
    } else if let Some(gist) = text
        .lines()
        .map(str::trim)
        .find(|l| l.chars().any(char::is_alphanumeric))
    {
        preview = format!("{preview} — {gist}");
    }
    e.preview = Some(preview);
    if !calls.is_empty() {
        e.detail.push_str("\n── calls");
        for c in calls {
            let mark = if c["ok"].as_bool() == Some(false) { "✗" } else { "✓" };
            let took = c["durationMs"].as_u64().map(fmt_ms).unwrap_or_default();
            let line = format!("\n{mark} {} {took}  {}", one(&c["name"]), summarize_args(&c["args"]));
            e.detail.push_str(&line);
        }
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
