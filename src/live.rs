use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::store::home;
use crate::text::sanitize;
use crate::transcript::Transcript;

const TAIL_BYTES: u64 = 64 * 1024;
pub const KEEP_LINES: usize = 400;

fn fd_prefix(w: &str) -> bool {
    w == "&" || (!w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()))
}

fn words(cmd: &str) -> Vec<String> {
    fn flush(cur: &mut String, out: &mut Vec<String>) {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => quote = Some(c),
            (None, '\\') => cur.extend(chars.next()),
            (None, c) if c.is_whitespace() && c != '\n' => flush(&mut cur, &mut out),
            (None, ';' | '|' | '(' | ')' | '\n') => {
                flush(&mut cur, &mut out);
                out.push(c.to_string());
            }
            (None, '&') if chars.peek() == Some(&'&') => {
                chars.next();
                flush(&mut cur, &mut out);
                out.push("&&".into());
            }
            (None, '>') => {
                if fd_prefix(&cur) {
                    cur.clear();
                } else {
                    flush(&mut cur, &mut out);
                }
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    chars.next();
                    while chars.peek().is_some_and(|d| d.is_ascii_digit() || *d == '-') {
                        chars.next();
                    }
                } else {
                    out.push(">".into());
                }
            }
            (None, c) => cur.push(c),
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn is_op(w: &str) -> bool {
    matches!(w, ";" | "|" | "&&" | "&" | "(" | ")" | ">" | "\n")
}

fn resolve(base: &Path, word: &str) -> Option<PathBuf> {
    if word.is_empty() || word.contains(['$', '`', '*']) || word == "/dev/null" || word.starts_with('-') {
        return None;
    }
    Some(match word.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => base.join(word),
    })
}

pub fn targets(cmd: &str, root: &Path) -> Vec<PathBuf> {
    let w = words(cmd);
    let mut base = root.to_path_buf();
    let mut out: Vec<PathBuf> = Vec::new();
    for i in 0..w.len() {
        let at_start = i == 0 || (is_op(&w[i - 1]) && w[i - 1] != ">");
        let next = w.get(i + 1).filter(|n| !is_op(n));
        let found = match w[i].as_str() {
            "cd" if at_start => {
                if let Some(p) = next.and_then(|n| resolve(&base, n)) {
                    base = p;
                }
                None
            }
            ">" => next.and_then(|n| resolve(&base, n)),
            "tee" if at_start => w[i + 1..]
                .iter()
                .take_while(|n| !is_op(n))
                .find(|n| !n.starts_with('-'))
                .and_then(|n| resolve(&base, n)),
            word if !at_start && (word.ends_with(".log") || word.ends_with(".out")) => resolve(&base, word),
            _ => None,
        };
        if let Some(p) = found
            && !out.contains(&p)
        {
            out.push(p);
        }
    }
    out
}

pub struct Live {
    pub entry: usize,
    candidates: Vec<PathBuf>,
    pub path: Option<PathBuf>,
    stamp: Option<(SystemTime, u64)>,
    pub size: u64,
    pub lines: Vec<String>,
}

impl Live {
    fn new(entry: usize, candidates: Vec<PathBuf>) -> Self {
        Self {
            entry,
            candidates,
            path: None,
            stamp: None,
            size: 0,
            lines: Vec::new(),
        }
    }

    fn poll(&mut self) -> bool {
        let newest = self
            .candidates
            .iter()
            .filter_map(|p| {
                let m = fs::metadata(p).ok().filter(|m| m.is_file())?;
                Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), p))
            })
            .max_by_key(|(t, ..)| *t);
        let Some((mtime, len, path)) = newest else {
            let changed = self.path.is_some();
            self.path = None;
            self.stamp = None;
            self.lines.clear();
            return changed;
        };
        let stamp = Some((mtime, len));
        if self.path.as_deref() == Some(path.as_path()) && self.stamp == stamp {
            return false;
        }
        self.lines = read_tail(path, len).unwrap_or_else(|e| vec![format!("(cannot read: {e})")]);
        self.path = Some(path.clone());
        self.stamp = stamp;
        self.size = len;
        true
    }
}

fn read_tail(path: &Path, len: u64) -> std::io::Result<Vec<String>> {
    let mut file = fs::File::open(path)?;
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| sanitize(l.trim_end_matches('\r').rsplit('\r').next().unwrap_or("")))
        .collect();
    if start > 0 {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let skip = lines.len().saturating_sub(KEEP_LINES);
    lines.drain(..skip);
    Ok(lines)
}

pub fn sync(live: &mut Option<Live>, t: &Transcript, root: &Path) -> bool {
    let Some(i) = t.running_index().filter(|&i| !t.entries[i].command.is_empty()) else {
        return live.take().is_some();
    };
    let mut changed = false;
    if live.as_ref().is_none_or(|l| l.entry != i) {
        let found = targets(&t.entries[i].command, t.cwd.as_deref().unwrap_or(root));
        changed = live.is_some();
        *live = (!found.is_empty()).then(|| Live::new(i, found));
    }
    changed | live.as_mut().is_some_and(Live::poll)
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
