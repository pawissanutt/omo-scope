use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Hit};
use crate::config::Config;
use crate::stats::{CostMode, Stats};
use crate::text::{clip, sanitize};
use crate::ui::{BOLD, Bar, DIM, SEL_FOCUS};

const WIDTH: u16 = 66;
const NAME_W: usize = 14;

#[derive(Debug, Default)]
pub struct Settings {
    pub selected: usize,
    pub status: Option<String>,
}

/// Config edits shared by keys and clicks; indices point into `Config::order`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Select(usize),
    Bar(usize),
    Row(usize),
    Up(usize),
    Down(usize),
    Cost,
    CtxDown,
    CtxUp,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Op(Op),
    Save,
    Close,
}

pub fn action(selected: usize, code: KeyCode) -> Option<Action> {
    let op = match code {
        KeyCode::Esc | KeyCode::Char('c' | 'q') => return Some(Action::Close),
        KeyCode::Enter | KeyCode::Char('S') => return Some(Action::Save),
        KeyCode::Up | KeyCode::Char('k') => Op::Select(selected.saturating_sub(1)),
        KeyCode::Down | KeyCode::Char('j') => Op::Select(selected.saturating_add(1)),
        KeyCode::Char(' ' | 'b') => Op::Bar(selected),
        KeyCode::Char('w') => Op::Row(selected),
        KeyCode::Char('K') => Op::Up(selected),
        KeyCode::Char('J') => Op::Down(selected),
        KeyCode::Char('m') => Op::Cost,
        KeyCode::Char('[' | '-') => Op::CtxDown,
        KeyCode::Char(']' | '+') => Op::CtxUp,
        KeyCode::Char('R') => Op::Reset,
        _ => return None,
    };
    Some(Action::Op(op))
}

/// Returns true when the config changed.
pub fn apply(s: &mut Settings, cfg: &mut Config, model: Option<&str>, op: Op) -> bool {
    let last = cfg.order.len().saturating_sub(1);
    match op {
        Op::Select(i) => {
            s.selected = i.min(last);
            false
        }
        Op::Bar(i) | Op::Row(i) => {
            let Some(&key) = cfg.order.get(i) else { return false };
            if matches!(op, Op::Bar(_)) {
                cfg.toggle_bar(key);
            } else {
                cfg.toggle_row(key);
            }
            s.selected = i;
            true
        }
        Op::Up(i) | Op::Down(i) => {
            s.selected = cfg.move_key(i, matches!(op, Op::Up(_))).min(last);
            true
        }
        Op::Cost => {
            cfg.cost = cfg.cost.next();
            true
        }
        Op::CtxDown | Op::CtxUp => {
            let Some(model) = model else { return false };
            cfg.step_ctx_limit(model, op == Op::CtxUp);
            true
        }
        Op::Reset => {
            cfg.reset();
            s.selected = s.selected.min(cfg.order.len().saturating_sub(1));
            s.status = None;
            true
        }
    }
}

pub fn on_key(app: &mut App, k: KeyEvent) {
    let Some(selected) = app.settings.as_ref().map(|s| s.selected) else {
        return;
    };
    match action(selected, k.code) {
        Some(Action::Close) => app.settings = None,
        Some(Action::Save) => app.save_settings(),
        Some(Action::Op(op)) => app.settings_op(op),
        None => {}
    }
}

fn pad(text: &str, w: usize) -> String {
    let text = clip(text, w);
    let n = w.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(n))
}

fn check(on: bool) -> &'static str {
    if on { "[x]" } else { "[ ]" }
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    app.hits.clear();
    let Some(selected) = app.settings.as_ref().map(|s| s.selected) else {
        return;
    };
    let status = app.settings.as_ref().and_then(|s| s.status.clone());
    let task = app.log.as_ref().and_then(|l| app.store.task(&l.task));
    let stats = task.map(|t| app.current_stats(t)).unwrap_or_default();
    let model = app.log_model();
    let ctx_limit = model.as_deref().and_then(|m| app.config.ctx_limit(m));
    let cfg = &app.config;

    let rows = cfg.order.len() as u16;
    let want = rows.saturating_add(9);
    let w = area.width.min(WIDTH);
    let h = area.height.min(want);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let rect = Rect::new(x, y, w, h);
    f.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .title(" Stats ")
        .title_style(Style::new().add_modifier(BOLD));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut hits = Vec::new();
    let mut lines = 0_u16;
    let mut line = |hits: &mut Vec<(Rect, Hit)>, base: Style, fill: &mut dyn FnMut(&mut Bar)| {
        if lines >= inner.height {
            return;
        }
        let mut bar = Bar::new(Rect::new(inner.x, inner.y + lines, inner.width, 1));
        fill(&mut bar);
        hits.extend(bar.finish(f, base));
        lines += 1;
    };

    let preview_w = (inner.width as usize).saturating_sub(NAME_W + 6 + 8 + 5);
    line(&mut hits, DIM, &mut |b| {
        b.push(
            format!(
                " {}{}{}{}",
                pad("stat", NAME_W - 1),
                pad("bar", 6),
                pad("row", 8),
                "preview"
            ),
            DIM,
            None,
        );
    });
    for (i, &key) in cfg.order.iter().enumerate() {
        let base = if i == selected { SEL_FOCUS } else { Style::default() };
        let preview = stats
            .render(key, cfg.cost, ctx_limit)
            .map(|s| sanitize(&s))
            .unwrap_or_else(|| "\u{2014}".into());
        line(&mut hits, base, &mut |b| {
            b.push(
                format!(" {}", pad(key.title(), NAME_W - 1)),
                base,
                Some(Hit::StatSelect(i)),
            );
            b.push(check(cfg.bar.contains(&key)), base, Some(Hit::StatBar(i)));
            b.push("   ", base, None);
            b.push(check(cfg.row.contains(&key)), base, Some(Hit::StatRow(i)));
            b.push("     ", base, None);
            b.push(pad(&preview, preview_w), base.fg(Color::Gray), Some(Hit::StatSelect(i)));
            b.push(" \u{2191}", base.fg(Color::Cyan), Some(Hit::StatUp(i)));
            b.push(" \u{2193}", base.fg(Color::Cyan), Some(Hit::StatDown(i)));
        });
    }
    line(&mut hits, Style::default(), &mut |_| {});
    line(&mut hits, Style::default(), &mut |b| {
        b.push(
            " cost on subscriptions   \u{2039} ",
            Style::default(),
            Some(Hit::CostMode),
        );
        for (n, mode) in [CostMode::Auto, CostMode::Always, CostMode::Never]
            .into_iter()
            .enumerate()
        {
            if n > 0 {
                b.push(" | ", DIM, Some(Hit::CostMode));
            }
            let style = if mode == cfg.cost {
                Style::new().fg(Color::Cyan).add_modifier(BOLD)
            } else {
                DIM
            };
            b.push(mode.name(), style, Some(Hit::CostMode));
        }
        b.push(" \u{203a}", Style::default(), Some(Hit::CostMode));
    });
    line(&mut hits, Style::default(), &mut |b| match model.as_deref() {
        Some(m) => {
            let limit = ctx_limit.map_or_else(|| "none".to_string(), Stats::fmt_count);
            let source = if cfg.ctx_is_auto(m) { "auto" } else { "set" };
            let tail = format!("  {limit}  [\u{2212}] [+]  ({source})");
            let room = (inner.width as usize).saturating_sub(tail.width() + 20);
            b.push(format!(" context limit for {}", clip(m, room)), Style::default(), None);
            b.push(format!("  {limit}  "), Style::new().add_modifier(BOLD), None);
            b.push("[\u{2212}]", Style::new().fg(Color::Cyan), Some(Hit::CtxDown));
            b.push(" ", Style::default(), None);
            b.push("[+]", Style::new().fg(Color::Cyan), Some(Hit::CtxUp));
            b.push(format!("  ({source})"), DIM, None);
        }
        None => b.push(" context limit: no task selected", DIM, None),
    });
    line(&mut hits, Style::default(), &mut |_| {});
    line(&mut hits, Style::default(), &mut |b| {
        let button = Style::new().fg(Color::Cyan).add_modifier(BOLD);
        b.push(" ", Style::default(), None);
        b.push("[ Save ]", button, Some(Hit::SaveSettings));
        b.push("   ", Style::default(), None);
        b.push("[ Reset defaults ]", button, Some(Hit::ResetSettings));
        b.push("   ", Style::default(), None);
        b.push("[ Close ]", button, Some(Hit::CloseSettings));
    });
    if let Some(status) = status {
        line(&mut hits, DIM, &mut |b| {
            b.push(format!(" {}", sanitize(&status)), DIM, None)
        });
    }
    app.hits.extend(hits);
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
