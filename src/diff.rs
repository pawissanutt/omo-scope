use std::path::Path;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use serde_json::Value;

use crate::text::{clip, sanitize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    File,
    Hunk,
    Add,
    Del,
    Ctx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub mark: Mark,
    pub text: String,
}

pub struct Change {
    pub summary: String,
    pub lines: Vec<DiffLine>,
}

fn line(mark: Mark, text: &str) -> DiffLine {
    DiffLine {
        mark,
        text: sanitize(text),
    }
}

pub(crate) fn short(path: &str, cwd: Option<&Path>) -> String {
    let rel = cwd
        .and_then(|c| Path::new(path).strip_prefix(c).ok())
        .map(|p| p.to_string_lossy().into_owned());
    sanitize(&rel.filter(|r| !r.is_empty()).unwrap_or_else(|| path.to_string()))
}

fn first_str<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| v[*k].as_str())
}

pub fn from_call(name: &str, args: &Value, cwd: Option<&Path>) -> Option<Change> {
    let mut lines = Vec::new();
    let path = first_str(args, &["path", "file_path", "filePath"]).map(|p| short(p, cwd));
    let patch = first_str(args, &["input", "patch"]).filter(|_| name.contains("patch"));
    if let Some(patch) = patch {
        patch_lines(patch, cwd, &mut lines);
    } else if let (Some(p), Some(content)) = (&path, args["content"].as_str()) {
        lines.push(line(Mark::File, &format!("write {p}")));
        lines.extend(content.lines().map(|l| line(Mark::Add, l)));
    } else if let Some(p) = &path {
        let edits: Vec<&Value> = match args["edits"].as_array() {
            Some(list) => list.iter().collect(),
            None => vec![args],
        };
        for e in edits {
            let old = first_str(e, &["oldText", "old_string", "old_str"]);
            let new = first_str(e, &["newText", "new_string", "new_str"]);
            if old.is_none() && new.is_none() {
                continue;
            }
            let head = if lines.is_empty() {
                line(Mark::File, &format!("edit {p}"))
            } else {
                line(Mark::Hunk, "@@")
            };
            lines.push(head);
            lines.extend(old.unwrap_or("").lines().map(|l| line(Mark::Del, l)));
            lines.extend(new.unwrap_or("").lines().map(|l| line(Mark::Add, l)));
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(Change {
        summary: summarize(&lines),
        lines,
    })
}

const FILE_OPS: [(&str, &str); 3] = [
    ("*** Add File: ", "add"),
    ("*** Delete File: ", "delete"),
    ("*** Update File: ", "update"),
];

fn patch_lines(patch: &str, cwd: Option<&Path>, out: &mut Vec<DiffLine>) {
    for raw in patch.lines() {
        if let Some((op, p)) = FILE_OPS
            .iter()
            .find_map(|(pre, op)| raw.strip_prefix(pre).map(|p| (op, p)))
        {
            out.push(line(Mark::File, &format!("{op} {}", short(p.trim(), cwd))));
        } else if let Some(to) = raw.strip_prefix("*** Move to: ") {
            if let Some(file) = out.iter_mut().rev().find(|l| l.mark == Mark::File) {
                file.text.push_str(&format!(" -> {}", short(to.trim(), cwd)));
            }
        } else if raw.starts_with("***") {
            continue;
        } else if raw.starts_with("@@") {
            out.push(line(Mark::Hunk, raw));
        } else if let Some(t) = raw.strip_prefix('+') {
            out.push(line(Mark::Add, t));
        } else if let Some(t) = raw.strip_prefix('-') {
            out.push(line(Mark::Del, t));
        } else {
            out.push(line(Mark::Ctx, raw.strip_prefix(' ').unwrap_or(raw)));
        }
    }
}

fn summarize(lines: &[DiffLine]) -> String {
    let count = |m: Mark| lines.iter().filter(|l| l.mark == m).count();
    let files: Vec<&str> = lines
        .iter()
        .filter(|l| l.mark == Mark::File)
        .map(|l| l.text.as_str())
        .collect();
    let head = match files.as_slice() {
        [one] => one.to_string(),
        many => format!("{} files", many.len()),
    };
    format!("{head}  +{} -{}", count(Mark::Add), count(Mark::Del))
}

pub fn render(diff: &[DiffLine], width: usize, limit: usize, out: &mut Vec<Line<'static>>) {
    let single = diff.iter().filter(|d| d.mark == Mark::File).count() <= 1;
    let shown: Vec<&DiffLine> = diff.iter().filter(|d| !(single && d.mark == Mark::File)).collect();
    for d in shown.iter().take(limit) {
        let (prefix, style) = match d.mark {
            Mark::File => ("", Style::new().add_modifier(Modifier::BOLD)),
            Mark::Hunk => ("", Style::new().fg(Color::Cyan)),
            Mark::Add => ("+ ", Style::new().fg(Color::Green)),
            Mark::Del => ("- ", Style::new().fg(Color::Red)),
            Mark::Ctx => ("  ", Style::new().fg(Color::DarkGray)),
        };
        out.push(Line::styled(clip(&format!("      {prefix}{}", d.text), width), style));
    }
    if shown.len() > limit {
        let more = format!("      ... {} more lines (click)", shown.len() - limit);
        out.push(Line::styled(more, Style::new().fg(Color::DarkGray)));
    }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
