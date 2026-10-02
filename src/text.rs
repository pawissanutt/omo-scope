use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Strip ANSI escape sequences and control characters, expand tabs, drop `\r`.
pub fn sanitize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                // CSI: parameters until a final byte in '@'..='~'.
                Some('[') => {
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                // OSC: until BEL or ST (ESC \).
                Some(']') => {
                    while let Some(n) = chars.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\t' => out.push_str("    "),
            '\n' => out.push('\n'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut line_w = 0;
        let mut last_space: Option<usize> = None;
        for c in raw.chars() {
            let cw = c.width().unwrap_or(0);
            if line_w + cw > width && !line.is_empty() {
                if c == ' ' {
                    out.push(std::mem::take(&mut line));
                    line_w = 0;
                    last_space = None;
                    continue;
                }
                match last_space {
                    Some(at) if at > 0 => {
                        let rest = line[at + 1..].to_string();
                        line.truncate(at);
                        out.push(std::mem::replace(&mut line, rest));
                    }
                    _ => out.push(std::mem::take(&mut line)),
                }
                line_w = line.width();
                last_space = line.rfind(' ');
                if line_w + cw > width && !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                    line_w = 0;
                    last_space = None;
                }
            }
            if c == ' ' {
                last_space = Some(line.len());
            }
            line.push(c);
            line_w += cw;
        }
        out.push(line);
    }
    out
}

pub fn clip(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > width - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

pub fn first_line(s: &str) -> &str {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

pub fn fmt_duration(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
    }
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
