use std::collections::HashSet;

use jiff::Timestamp;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus, Hit, LogLine, Reasoning, Row, Tab};
use crate::live::{KEEP_LINES, Live};
use crate::store::{Store, Task};
use crate::text::{clip, first_line, fmt_duration, wrap};
use crate::transcript::{Entry, Kind, Link};

pub(crate) const DIM: Style = Style::new().fg(Color::DarkGray);
const BAR: Style = Style::new().bg(Color::Indexed(236));
const SEL: Style = Style::new().bg(Color::Indexed(238));
const PIN: Style = Style::new().bg(Color::Indexed(235));
pub(crate) const SEL_FOCUS: Style = Style::new().bg(Color::Indexed(24));
pub(crate) const BOLD: Modifier = Modifier::BOLD;

pub(crate) struct Bar {
    area: Rect,
    x: u16,
    spans: Vec<Span<'static>>,
    hits: Vec<(Rect, Hit)>,
}

impl Bar {
    pub(crate) fn new(area: Rect) -> Self {
        Self {
            area,
            x: area.x,
            spans: Vec::new(),
            hits: Vec::new(),
        }
    }

    fn room(&self) -> usize {
        self.area.right().saturating_sub(self.x) as usize
    }

    pub(crate) fn push(&mut self, text: impl Into<String>, style: Style, hit: Option<Hit>) {
        let text = clip(&text.into(), self.room());
        let w = text.width() as u16;
        if w == 0 {
            return;
        }
        if let Some(h) = hit {
            self.hits.push((Rect::new(self.x, self.area.y, w, 1), h));
        }
        self.x += w;
        self.spans.push(Span::styled(text, style));
    }

    fn pad_to(&mut self, right: usize) {
        let n = self.room().saturating_sub(right);
        self.push(" ".repeat(n), Style::default(), None);
    }

    fn fill_to(&mut self, right: usize, style: Style) {
        let n = self.room().saturating_sub(right);
        if n > 0 {
            self.push(format!(" {}", "─".repeat(n - 1)), style, None);
        }
    }

    pub(crate) fn finish(self, f: &mut Frame, base: Style) -> Vec<(Rect, Hit)> {
        f.render_widget(Paragraph::new(Line::from(self.spans)).style(base), self.area);
        self.hits
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits.clear();
    let area = f.area();
    if area.height < 4 || area.width < 20 {
        f.render_widget(Paragraph::new("omo-scope: pane too small"), area);
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    app.body_area = body;
    let list_h = if app.zoom {
        0
    } else if let Some(h) = app.split {
        h.clamp(1, body.height.saturating_sub(2).max(1))
    } else {
        let hi = (body.height * 2 / 5).max(3).min(body.height);
        let lo = 3.min(body.height);
        (app.rows.len().min(u16::MAX as usize) as u16).clamp(lo, hi)
    };
    let list = Rect::new(body.x, body.y, body.width, list_h);
    let log = Rect::new(body.x, body.y + list_h, body.width, body.height - list_h);
    app.list_area = list;
    app.log_area = log;
    draw_header(f, app, header);
    if list_h > 0 {
        draw_list(f, app, list);
    }
    draw_log(f, app, log);
    draw_footer(f, app, footer);
    if app.picker.is_some() {
        draw_picker(f, app, area);
    }
    if app.settings.is_some() {
        crate::settings::draw(f, app, area);
    }
}

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let (tasks, runs) = match app.session.as_deref() {
        Some(s) => (app.store.session_tree(s).len(), app.store.runs_for(s).len()),
        None => (0, 0),
    };
    let tabs = [
        (Tab::Tasks, format!(" Tasks {tasks} ")),
        (Tab::Dag, format!(" DAG {runs} ")),
    ];
    let tabs_w: usize = tabs.iter().map(|(_, s)| s.width()).sum::<usize>() + 1;
    let mut bar = Bar::new(area);
    bar.push(
        " omo-scope ",
        Style::new().fg(Color::Black).bg(Color::Cyan).add_modifier(BOLD),
        None,
    );
    let room = bar.room().saturating_sub(tabs_w + 1);
    let title = format!(" {} ▾", clip(&app.session_title, room.saturating_sub(3)));
    bar.push(title, BAR.add_modifier(BOLD), Some(Hit::Picker));
    bar.pad_to(tabs_w);
    for (t, s) in tabs {
        let style = if app.tab == t {
            Style::new().fg(Color::Black).bg(Color::White)
        } else {
            BAR.fg(Color::Gray)
        };
        bar.push(s, style, Some(Hit::Tab(t)));
    }
    let hits = bar.finish(f, BAR);
    app.hits.extend(hits);
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let h = area.height as usize;
    if app.reveal {
        if app.selected < app.list_scroll {
            app.list_scroll = app.selected;
        } else if app.selected >= app.list_scroll + h {
            app.list_scroll = app.selected + 1 - h;
        }
        app.reveal = false;
    }
    app.list_scroll = app.list_scroll.min(app.rows.len().saturating_sub(h));
    let focused = app.focus == Focus::List && !app.zoom;
    let mut hits = Vec::new();
    for (i, row) in app.rows.iter().enumerate().skip(app.list_scroll).take(h) {
        let r = Rect::new(area.x, area.y + (i - app.list_scroll) as u16, area.width, 1);
        let style = match (i == app.selected, focused) {
            (true, true) => SEL_FOCUS,
            (true, false) => SEL,
            _ => Style::default(),
        };
        f.render_widget(Paragraph::new(row_line(app, row, area.width as usize)).style(style), r);
        hits.push((r, Hit::Row(i)));
    }
    app.hits.extend(hits);
}

fn row_line(app: &App, row: &Row, width: usize) -> Line<'static> {
    match row {
        Row::Task { id, depth } => match app.store.task(id) {
            Some(t) => {
                let extras = vec![t.category.clone(), t.model.clone()];
                let meta = row_meta(app, t, extras, width.saturating_sub(*depth * 2 + 15));
                lay(
                    *depth * 2,
                    status_icon(&t.status),
                    &t.label,
                    Style::default(),
                    meta,
                    width,
                )
            }
            None => Line::styled(format!(" {id}"), DIM),
        },
        Row::Run { id } => match app.store.run(id) {
            Some(r) => {
                let (done, total) = r.progress();
                let meta = format!("{} {done}/{total} ", r.status);
                lay(
                    0,
                    status_icon(&r.status),
                    &r.name,
                    Style::new().add_modifier(BOLD),
                    meta,
                    width,
                )
            }
            None => Line::styled(format!(" {id}"), DIM),
        },
        Row::Node { run, node } => {
            let Some(n) = app.store.run(run).and_then(|r| r.nodes.iter().find(|n| &n.id == node)) else {
                return Line::styled(format!("   {node}"), DIM);
            };
            let deps = (!n.depends_on.is_empty()).then(|| format!("<- {}", n.depends_on.join(",")));
            let room = width.saturating_sub(17);
            let meta = match n.task_id.as_deref().and_then(|t| app.store.task(t)) {
                Some(t) => row_meta(app, t, deps.into_iter().collect(), room),
                None => fit_meta(deps.into_iter().collect(), Vec::new(), String::new(), room),
            };
            let label = format!("{} {}", n.id, n.label);
            lay(2, status_icon(&n.state), &label, Style::default(), meta, width)
        }
        Row::Info(s) => Line::styled(format!(" {s}"), DIM),
    }
}

fn row_meta(app: &App, t: &Task, extras: Vec<String>, room: usize) -> String {
    let cfg = &app.config;
    let stats = app.current_stats(t);
    let limit = cfg.ctx_limit(&t.model);
    let rendered = cfg
        .row_keys()
        .into_iter()
        .filter_map(|k| stats.render(k, cfg.cost, limit))
        .collect();
    let elapsed = t.elapsed_secs().map(fmt_duration).unwrap_or_default();
    fit_meta(extras, rendered, elapsed, room)
}

/// Drops `extras` first, then stats from the end of the list, so elapsed time stays longest.
fn fit_meta(mut extras: Vec<String>, mut stats: Vec<String>, elapsed: String, room: usize) -> String {
    loop {
        let parts: Vec<&str> = extras
            .iter()
            .chain(&stats)
            .chain(std::iter::once(&elapsed))
            .map(String::as_str)
            .filter(|p| !p.is_empty())
            .collect();
        let meta = format!("{} ", parts.join(" · "));
        if meta.width() <= room {
            return meta;
        }
        if !extras.is_empty() {
            extras.remove(0);
        } else if stats.pop().is_none() {
            return meta;
        }
    }
}

fn lay(
    indent: usize,
    icon: (&'static str, Color),
    label: &str,
    style: Style,
    meta: String,
    width: usize,
) -> Line<'static> {
    let lead = format!("{}{} ", " ".repeat(indent + 1), icon.0);
    let meta = if lead.width() + meta.width() + 12 > width {
        String::new()
    } else {
        meta
    };
    let label = clip(label, width.saturating_sub(lead.width() + meta.width() + 1));
    let pad = width.saturating_sub(lead.width() + label.width() + meta.width());
    Line::from(vec![
        Span::styled(lead, Style::new().fg(icon.1)),
        Span::styled(label, style),
        Span::raw(" ".repeat(pad)),
        Span::styled(meta, DIM),
    ])
}

fn status_icon(status: &str) -> (&'static str, Color) {
    match status {
        "running" | "resident" | "started" => ("●", Color::Cyan),
        "completed" | "succeeded" | "done" => ("✓", Color::Green),
        "error" | "failed" | "lost" => ("✗", Color::Red),
        "cancelled" | "skipped" => ("-", Color::DarkGray),
        "interrupted" | "blocked" | "suspended" => ("!", Color::Magenta),
        "pending" | "queued" | "scheduled" | "waiting" => ("○", Color::DarkGray),
        _ => ("?", Color::Yellow),
    }
}

fn draw_log(f: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let layout = title_layout(app, area.width as usize);
    let extra = layout.as_ref().map_or(&[][..], |(_, lines)| lines.as_slice());
    let head = 1 + (extra.len() as u16).min(area.height.saturating_sub(2));
    for (i, line) in extra.iter().take(usize::from(head - 1)).enumerate() {
        let row = Rect::new(area.x, area.y + 1 + i as u16, area.width, 1);
        f.render_widget(
            Paragraph::new(Line::styled(line.clone(), BAR.fg(Color::Gray))).style(BAR),
            row,
        );
    }
    let title = Rect::new(area.x, area.y, area.width, head);
    draw_log_title(f, app, title, layout.map(|(meta, _)| meta));
    let rest = area.height - head;
    let pins = app.log.as_ref().map_or_else(Vec::new, |log| {
        let room = if app.pin_open { (rest / 2).max(2) } else { 2 };
        let t = &log.transcript;
        let width = area.width.saturating_sub(1) as usize;
        let max = room.min(rest.saturating_sub(3)) as usize;
        crate::pinned::lines(t.plan.as_ref(), t.goal.as_ref(), app.pin_open, width, max)
    });
    let pin_h = pins.len() as u16;
    for (i, line) in pins.into_iter().enumerate() {
        let row = Rect::new(area.x, area.y + head + i as u16, area.width, 1);
        f.render_widget(Paragraph::new("").style(PIN), row);
        let text = Rect::new(row.x + 1, row.y, row.width.saturating_sub(1), 1);
        f.render_widget(Paragraph::new(line).style(PIN), text);
        app.hits.push((row, Hit::Pin));
    }
    let body = Rect::new(area.x, area.y + head + pin_h, area.width, rest - pin_h);
    let reasoning = app.reasoning;
    let Some(log) = app.log.as_mut() else {
        f.render_widget(
            Paragraph::new(Line::styled(" select a task to follow its transcript", DIM)),
            body,
        );
        return;
    };
    let width = body.width.saturating_sub(1).max(1);
    if log.dirty || log.width != width {
        log.lines = render_entries(
            &log.transcript.entries,
            &log.expanded,
            width as usize,
            reasoning,
            log.live.as_ref(),
            Some(&app.store),
        );
        log.width = width;
        log.dirty = false;
    }
    log.height = body.height as usize;
    let max = log.lines.len().saturating_sub(log.height);
    log.offset = if log.follow { max } else { log.offset.min(max) };
    if log.lines.is_empty() {
        let msg = match &log.error {
            Some(e) => format!(" {e}"),
            None => " waiting for transcript...".to_string(),
        };
        f.render_widget(Paragraph::new(Line::styled(msg, DIM)), body);
        return;
    }
    let mut hits = Vec::new();
    for (i, l) in log.lines.iter().skip(log.offset).take(log.height).enumerate() {
        let y = body.y + i as u16;
        f.render_widget(Paragraph::new(l.line.clone()), Rect::new(body.x + 1, y, width, 1));
        let row = Rect::new(body.x, y, body.width, 1);
        if let Some(t) = &l.task {
            hits.push((row, Hit::Task(t.clone())));
        } else if let Some(e) = l.entry {
            hits.push((row, Hit::Entry(e)));
        }
    }
    app.hits.extend(hits);
}

/// Title-line meta plus extra lines: everything stays on one line when it fits; otherwise the title
/// keeps the status (and model/elapsed when they fit) and the rest wraps below for narrow panes.
fn title_layout(app: &App, width: usize) -> Option<(String, Vec<String>)> {
    let log = app.log.as_ref()?;
    let t = app.store.task(&log.task)?;
    let mut fixed = vec![t.status.clone()];
    fixed.extend((!t.model.is_empty()).then(|| t.model.clone()));
    fixed.extend(t.elapsed_secs().map(fmt_duration));
    fixed.extend((log.bad > 0).then(|| format!("{} bad lines", log.bad)));
    let cfg = &app.config;
    let stats = app.current_stats(t);
    let limit = cfg.ctx_limit(&t.model);
    let shown: Vec<String> = cfg
        .bar_keys()
        .into_iter()
        .filter_map(|k| stats.render(k, cfg.cost, limit))
        .collect();
    let fits = |m: &str| m.width() + TITLE_TAIL + 20 <= width;
    let join = |parts: &[String]| format!(" {} ", parts.join(" · "));
    let all = join(&[fixed.as_slice(), shown.as_slice()].concat());
    if fits(&all) {
        return Some((all, Vec::new()));
    }
    let keep = if fits(&join(&fixed)) { fixed.len() } else { 1 };
    let rest = [&fixed[keep..], shown.as_slice()].concat();
    Some((join(&fixed[..keep]), wrap_parts(&rest, width)))
}

fn wrap_parts(parts: &[String], width: usize) -> Vec<String> {
    let room = width.saturating_sub(2).max(1);
    let mut lines = Vec::new();
    let mut cur = String::new();
    for p in parts {
        let next = if cur.is_empty() {
            p.clone()
        } else {
            format!("{cur} · {p}")
        };
        if cur.is_empty() || next.width() <= room {
            cur = next;
        } else {
            lines.push(std::mem::replace(&mut cur, p.clone()));
        }
    }
    lines.extend((!cur.is_empty()).then_some(cur));
    lines.into_iter().map(|l| format!(" {}", clip(&l, room))).collect()
}

const TITLE_TAIL: usize = 11;

fn draw_log_title(f: &mut Frame, app: &mut App, area: Rect, meta: Option<String>) {
    let line = Rect::new(area.x, area.y, area.width, 1);
    let focused = app.focus == Focus::Log || app.zoom;
    let base = if focused { BAR.add_modifier(BOLD) } else { BAR };
    let base = if app.dragging {
        base.bg(Color::Indexed(24))
    } else {
        base
    };
    let rule = base.fg(Color::DarkGray);
    let zoom = if app.zoom { " [list] " } else { " [zoom] " };
    let gear = " ⚙ ";
    let tail_w = gear.width() + zoom.width();
    let mut bar = Bar::new(line);
    if !app.zoom {
        bar.push(" ≡", rule, None);
    }
    let current = app.log.as_ref().and_then(|l| app.store.task(&l.task)).zip(meta);
    match current {
        Some((t, meta)) => {
            let (icon, color) = status_icon(&t.status);
            bar.push(format!(" {icon} "), base.fg(color), None);
            let room = bar.room().saturating_sub(meta.width() + tail_w);
            bar.push(clip(&format!("{} {}", t.id, t.label), room), base, None);
            bar.fill_to(meta.width() + tail_w, rule);
            bar.push(meta, base.fg(Color::Gray), None);
        }
        None => {
            bar.push(" transcript", base, None);
            bar.fill_to(tail_w, rule);
        }
    }
    bar.push(gear, base.fg(Color::Cyan), Some(Hit::Settings));
    bar.push(zoom, base.fg(Color::Cyan), Some(Hit::Zoom));
    let hits = bar.finish(f, base);
    if !app.zoom {
        app.hits.push((area, Hit::Splitter));
    }
    app.hits.extend(hits);
}

const OPEN_LIMIT: usize = 2000;
const DIFF_PREVIEW: usize = 6;

fn block(out: &mut Vec<Line<'static>>, text: &str, indent: usize, width: usize, limit: usize, style: Style) {
    block_pad(out, text, &" ".repeat(indent), width, limit, style);
}

fn block_pad(out: &mut Vec<Line<'static>>, text: &str, pad: &str, width: usize, limit: usize, style: Style) {
    let wrapped = wrap(text, width.saturating_sub(pad.width()).max(1));
    let total = wrapped.len();
    for w in wrapped.into_iter().take(limit) {
        out.push(Line::styled(format!("{pad}{w}"), style));
    }
    if total > limit {
        out.push(Line::styled(
            format!("{pad}... {} more lines (click)", total - limit),
            DIM,
        ));
    }
}

pub fn render_entries(
    entries: &[Entry],
    expanded: &HashSet<usize>,
    width: usize,
    reasoning: Reasoning,
    live: Option<&Live>,
    store: Option<&Store>,
) -> Vec<LogLine> {
    let mut out = Vec::new();
    let think = Style::new().fg(Color::Indexed(246)).add_modifier(Modifier::ITALIC);
    for (i, e) in entries.iter().enumerate() {
        let open = expanded.contains(&i);
        if e.kind == Kind::Thinking && reasoning == Reasoning::Hidden {
            continue;
        }
        let mut lines: Vec<Line<'static>> = Vec::new();
        match e.kind {
            Kind::User => {
                lines.push(Line::default());
                lines.push(Line::styled("> user", Style::new().fg(Color::Cyan).add_modifier(BOLD)));
                let limit = if open { OPEN_LIMIT } else { 4 };
                block(&mut lines, &e.text, 2, width, limit, Style::new().fg(Color::Gray));
            }
            Kind::Assistant => {
                lines.push(Line::default());
                block(&mut lines, &e.text, 0, width, OPEN_LIMIT, Style::default());
            }
            Kind::Thinking => {
                let limit = if open || reasoning == Reasoning::Full {
                    OPEN_LIMIT
                } else {
                    3
                };
                block_pad(&mut lines, &e.text.replace("**", ""), "  │ ", width, limit, think);
            }
            Kind::Tool => tool_lines(&mut lines, e, open, width, live.filter(|l| l.entry == i)),
            Kind::Note => {
                let note = clip(&format!("  · {}: {}", e.title, first_line(&e.text)), width);
                lines.push(Line::styled(note, DIM));
            }
            Kind::Error => {
                let red = Style::new().fg(Color::Red);
                block(&mut lines, &format!("x {}: {}", e.title, e.text), 2, width, 20, red);
            }
            Kind::Summary => {
                let head = format!("  = {} ({} chars)", e.title, e.text.len());
                lines.push(Line::styled(head, Style::new().fg(Color::Magenta)));
                if open {
                    block(&mut lines, &e.text, 4, width, OPEN_LIMIT, Style::new().fg(Color::Gray));
                }
            }
        }
        let tag = e.collapsible().then_some(i);
        let mut lines = lines.into_iter();
        let entry = |line| LogLine {
            entry: tag,
            task: None,
            line,
        };
        out.extend(lines.next().map(entry));
        out.extend(e.links.iter().map(|l| LogLine {
            entry: None,
            task: Some(l.task.clone()),
            line: link_line(l, store, width),
        }));
        out.extend(lines.map(entry));
    }
    out
}

fn link_line(l: &Link, store: Option<&Store>, width: usize) -> Line<'static> {
    let task = store.and_then(|s| s.task(&l.task));
    let (icon, color) = status_icon(task.map_or("", |t| t.status.as_str()));
    let name = match task {
        _ if !l.name.is_empty() => l.name.clone(),
        Some(t) => t.label.clone(),
        None => l.task.clone(),
    };
    let mut meta: Vec<String> = task
        .map(|t| vec![t.status.clone(), t.category.clone()])
        .unwrap_or_default();
    meta.retain(|m| !m.is_empty());
    let mut spans = vec![
        Span::styled("      ⇢ ", DIM),
        Span::styled(format!("{icon} "), Style::new().fg(color)),
        Span::styled(name, Style::new().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)),
    ];
    if !meta.is_empty() {
        spans.push(Span::styled(format!(" · {}", meta.join(" · ")), DIM));
    }
    if !l.note.is_empty() {
        let style = Style::new().fg(if l.error { Color::Red } else { Color::Gray });
        spans.push(Span::styled(format!(" — {}", l.note), style));
    }
    let mut room = width;
    for s in &mut spans {
        let clipped = clip(&s.content, room);
        room = room.saturating_sub(clipped.width());
        s.content = clipped.into();
    }
    Line::from(spans)
}

fn tool_lines(lines: &mut Vec<Line<'static>>, e: &Entry, open: bool, width: usize, live: Option<&Live>) {
    let (mark, color) = match &e.result {
        None => ("●", Color::Cyan),
        Some(r) if r.is_error => ("✗", Color::Red),
        Some(_) => ("✓", Color::Green),
    };
    let lead = format!("  {mark} ");
    let title = clip(&e.title, 24);
    let rest = width.saturating_sub(lead.width() + title.width() + 1);
    lines.push(Line::from(vec![
        Span::styled(lead, Style::new().fg(color)),
        Span::styled(title, Style::new().fg(Color::Yellow).add_modifier(BOLD)),
        Span::raw(" "),
        Span::raw(clip(&e.text, rest)),
    ]));
    if let Some(l) = live {
        live_lines(lines, l, open, width);
    }
    if !e.diff.is_empty() {
        crate::diff::render(&e.diff, width, if open { OPEN_LIMIT } else { DIFF_PREVIEW }, lines);
        if let Some(r) = e.result.as_ref().filter(|r| r.is_error) {
            block(
                lines,
                &r.text,
                6,
                width,
                if open { 400 } else { 3 },
                Style::new().fg(Color::Red),
            );
        }
        return;
    }
    let out_style = match &e.result {
        Some(r) if r.is_error => Style::new().fg(Color::Red),
        _ => DIM,
    };
    if open {
        if !e.detail.is_empty() {
            block(lines, &e.detail, 6, width, OPEN_LIMIT, Style::new().fg(Color::Gray));
        }
        if let Some(r) = &e.result {
            lines.push(Line::styled("    ── result", DIM));
            block(lines, &r.text, 6, width, 400, out_style);
        }
    } else if let Some(r) = &e.result {
        let preview = match &e.preview {
            Some(p) if !r.is_error || !e.links.is_empty() => p.as_str(),
            _ => first_line(&r.text),
        };
        if !preview.is_empty() {
            lines.push(Line::styled(clip(&format!("      {preview}"), width), out_style));
        }
    }
}

fn live_lines(lines: &mut Vec<Line<'static>>, live: &Live, open: bool, width: usize) {
    let Some(path) = &live.path else {
        lines.push(Line::styled("      waiting for output file...", DIM));
        return;
    };
    let head = format!("    ── live {} ({}) ", path.display(), fmt_size(live.size));
    lines.push(Line::styled(clip(&head, width), Style::new().fg(Color::Cyan)));
    let n = if open { KEEP_LINES } else { 6 };
    let start = live.lines.len().saturating_sub(n);
    for l in &live.lines[start..] {
        lines.push(Line::styled(
            clip(&format!("      {l}"), width),
            Style::new().fg(Color::Gray),
        ));
    }
}

fn fmt_size(n: u64) -> String {
    match n {
        0..1024 => format!("{n} B"),
        1024..1_048_576 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

fn live_status(app: &App) -> Option<String> {
    let log = app.log.as_ref()?;
    let t = app.store.task(&log.task)?;
    if !t.is_active() {
        return t.error.as_ref().map(|e| format!("error: {}", first_line(e)));
    }
    let now = Timestamp::now().as_second();
    if let Some(tool) = log.transcript.running_tool() {
        let secs = tool.at.map(|a| now - a.as_second()).unwrap_or(0);
        return Some(format!("{} running {}", tool.title, fmt_duration(secs)));
    }
    let idle = log.transcript.last_at.map(|a| now - a.as_second())?;
    Some(format!("model working, last entry {} ago", fmt_duration(idle)))
}

fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    let mut bar = Bar::new(area);
    if let Some(log) = app.log.as_ref() {
        if log.follow {
            bar.push(" ▼ live ", BAR.fg(Color::Green), None);
        } else if log.unseen > 0 {
            let pill = format!(" ▼ {} new — follow ", log.unseen);
            bar.push(pill, Style::new().fg(Color::Black).bg(Color::Yellow), Some(Hit::Follow));
        } else {
            bar.push(" ‖ paused — follow ", BAR.fg(Color::Yellow), Some(Hit::Follow));
        }
    }
    if let Some(s) = live_status(app) {
        bar.push(format!(" {s} "), BAR.fg(Color::Cyan), None);
    }
    let mut hint = format!(
        " r:reasoning {} s:session t:tab z:zoom x:expand p:plan c:settings q:quit ",
        app.reasoning.label()
    );
    if app.store.invalid() > 0 {
        hint = format!(" {} unreadable files ·{hint}", app.store.invalid());
    }
    bar.pad_to(hint.width());
    bar.push(hint, BAR.fg(Color::DarkGray), None);
    let hits = bar.finish(f, BAR);
    app.hits.extend(hits);
}

fn draw_picker(f: &mut Frame, app: &mut App, area: Rect) {
    app.hits.clear();
    let rect = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(2));
    f.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .title(" sessions ")
        .title_style(Style::new().add_modifier(BOLD));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let close = Rect::new(rect.right().saturating_sub(6), rect.y, 5, 1);
    f.render_widget(Paragraph::new("[x]").style(Style::new().fg(Color::Cyan)), close);
    let mut hits = vec![(close, Hit::ClosePicker)];
    let Some(p) = app.picker.as_mut() else { return };
    let rows = inner.height as usize;
    if p.reveal {
        if p.selected < p.scroll {
            p.scroll = p.selected;
        } else if p.selected >= p.scroll + rows {
            p.scroll = p.selected + 1 - rows;
        }
        p.reveal = false;
    }
    p.scroll = p.scroll.min(p.sessions.len().saturating_sub(rows));
    if p.sessions.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled(" no sessions found for this project", DIM)),
            inner,
        );
    }
    let width = inner.width as usize;
    for (i, s) in p.sessions.iter().enumerate().skip(p.scroll).take(rows) {
        let r = Rect::new(inner.x, inner.y + (i - p.scroll) as u16, inner.width, 1);
        let current = app.session.as_deref() == Some(s.id.as_str());
        let mark = if current { "▸" } else { " " };
        let tasks = match (s.tasks, s.active) {
            (0, _) => String::new(),
            (n, 0) => format!("{n} tasks"),
            (n, a) => format!("{n} tasks, {a} live"),
        };
        let left = format!("{mark} {:<12} {:<17} ", fmt_time(s.modified), tasks);
        let title = if s.title.is_empty() {
            s.id.as_str()
        } else {
            s.title.as_str()
        };
        let title = clip(title, width.saturating_sub(left.width()));
        let style = if i == p.selected { SEL_FOCUS } else { Style::default() };
        let tstyle = if s.active > 0 {
            Style::new().fg(Color::Cyan)
        } else {
            DIM
        };
        let line = Line::from(vec![Span::styled(left, tstyle), Span::raw(title)]);
        f.render_widget(Paragraph::new(line).style(style), r);
        hits.push((r, Hit::Session(i)));
    }
    app.hits.extend(hits);
}

fn fmt_time(t: Option<Timestamp>) -> String {
    let Some(t) = t else { return "-".into() };
    let tz = jiff::tz::TimeZone::system();
    let z = t.to_zoned(tz.clone());
    let today = Timestamp::now().to_zoned(tz).date();
    if z.date() == today {
        z.strftime("%H:%M").to_string()
    } else {
        z.strftime("%b %d %H:%M").to_string()
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
