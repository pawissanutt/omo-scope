//! Plan (`senpi.todo-state`) and goal state pinned above the transcript.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::stats::Stats;
use crate::text::{clip, first_line, fmt_duration, sanitize, wrap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub content: String,
    pub status: String,
}

impl Step {
    fn closed(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "abandoned")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub phases: Vec<Phase>,
}

impl Plan {
    /// Parses the `data` of a `senpi.todo-state` record; an empty list (cleared plan) is `None`.
    pub fn from_state(data: &Value) -> Option<Self> {
        let phases: Vec<Phase> = data["phases"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| Phase {
                name: sanitize(p["name"].as_str().unwrap_or("")),
                steps: p["tasks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|t| Step {
                        content: sanitize(first_line(t["content"].as_str().unwrap_or(""))),
                        status: t["status"].as_str().unwrap_or("pending").to_string(),
                    })
                    .collect(),
            })
            .filter(|p| !p.steps.is_empty())
            .collect();
        (!phases.is_empty()).then_some(Self { phases })
    }

    pub fn progress(&self) -> (usize, usize) {
        let steps = || self.phases.iter().flat_map(|p| &p.steps);
        (steps().filter(|s| s.closed()).count(), steps().count())
    }

    pub fn current(&self) -> Option<(&Phase, &Step)> {
        let all = || self.phases.iter().flat_map(|p| p.steps.iter().map(move |s| (p, s)));
        all()
            .find(|(_, s)| s.status == "in_progress")
            .or_else(|| all().find(|(_, s)| !s.closed()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub objective: String,
    pub status: String,
    pub tokens: Option<u64>,
    pub secs: Option<i64>,
    pub reason: String,
    /// `goal-continuation` wakes seen since this goal was recorded.
    pub wakes: usize,
}

impl Goal {
    /// Parses the `goal` object of a create/update/get_goal result.
    pub fn from_json(g: &Value) -> Option<Self> {
        Some(Self {
            objective: sanitize(g["objective"].as_str()?.trim()),
            status: sanitize(g["status"].as_str().unwrap_or("active")),
            tokens: g["tokensUsed"].as_u64(),
            secs: g["timeUsedSeconds"].as_i64(),
            reason: sanitize(g["blockedReason"].as_str().unwrap_or("").trim()),
            wakes: 0,
        })
    }

    pub fn summary(&self) -> String {
        let mut parts = vec![self.status.clone()];
        parts.extend(self.tokens.map(|t| format!("{} tok", Stats::fmt_count(t))));
        parts.extend(self.secs.map(fmt_duration));
        parts.extend((self.wakes > 0).then(|| format!("woken ×{}", self.wakes)));
        parts.join(" · ")
    }

    fn color(&self) -> Color {
        match self.status.as_str() {
            "active" => Color::Cyan,
            "complete" | "completed" => Color::Green,
            "blocked" => Color::Red,
            _ => Color::Yellow,
        }
    }
}

const DIM: Style = Style::new().fg(Color::DarkGray);
const OBJECTIVE_LINES: usize = 4;

fn step_icon(status: &str) -> (&'static str, Style) {
    match status {
        "completed" => ("✓", Style::new().fg(Color::Green)),
        "in_progress" => ("●", Style::new().fg(Color::Cyan)),
        "abandoned" => ("-", DIM),
        _ => ("○", DIM),
    }
}

/// Lines for the pinned strip, at most `max` of them. Collapsed: one line each for goal and plan.
pub fn lines(plan: Option<&Plan>, goal: Option<&Goal>, open: bool, width: usize, max: usize) -> Vec<Line<'static>> {
    let arrow = if open { "▾ " } else { "▸ " };
    let mut out = Vec::new();
    if let Some(g) = goal {
        let head = format!("{arrow}◎ goal {}", g.summary());
        let tail = if g.status == "blocked" && !g.reason.is_empty() {
            &g.reason
        } else {
            &g.objective
        };
        let mut spans = vec![Span::styled(head, Style::new().fg(g.color()))];
        if !open {
            let used = spans[0].content.width() + 3;
            spans.push(Span::raw(format!(
                " · {}",
                clip(first_line(tail), width.saturating_sub(used))
            )));
        }
        out.push(Line::from(spans));
        if open {
            push_wrapped(&mut out, &g.objective, width, Style::new().fg(Color::Gray));
            if !g.reason.is_empty() {
                push_wrapped(&mut out, &g.reason, width, Style::new().fg(Color::Red));
            }
        }
    }
    if let Some(p) = plan {
        let (done, total) = p.progress();
        let head = format!("{arrow}plan {done}/{total}");
        let mut spans = vec![Span::styled(
            head,
            Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        )];
        if !open {
            match p.current() {
                Some((phase, step)) => {
                    let (icon, style) = step_icon(&step.status);
                    spans.push(Span::styled(format!(" · {} ▸ ", phase.name), DIM));
                    spans.push(Span::styled(format!("{icon} "), style));
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    spans.push(Span::raw(clip(&step.content, width.saturating_sub(used))));
                }
                None => spans.push(Span::styled(" · all done", Style::new().fg(Color::Green))),
            }
        }
        out.push(Line::from(spans));
        if open {
            plan_lines(&mut out, p, width);
        }
    }
    if out.len() > max {
        let hidden = out.len() + 1 - max;
        out.truncate(max.saturating_sub(1));
        out.push(Line::styled(
            format!("    ... {hidden} more lines (zoom for room)"),
            DIM,
        ));
        out.truncate(max);
    }
    out
}

fn plan_lines(out: &mut Vec<Line<'static>>, p: &Plan, width: usize) {
    for phase in &p.phases {
        let done = phase.steps.iter().filter(|s| s.closed()).count();
        let n = phase.steps.len();
        if done == n {
            out.push(Line::styled(
                clip(&format!("  ✓ {} {done}/{n}", phase.name), width),
                DIM,
            ));
            continue;
        }
        out.push(Line::styled(
            clip(&format!("  {} {done}/{n}", phase.name), width),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        for s in &phase.steps {
            let (icon, style) = step_icon(&s.status);
            let text_style = match s.status.as_str() {
                "in_progress" => Style::default(),
                "pending" => Style::new().fg(Color::Gray),
                _ => DIM,
            };
            out.push(Line::from(vec![
                Span::styled(format!("    {icon} "), style),
                Span::styled(clip(&s.content, width.saturating_sub(6)), text_style),
            ]));
        }
    }
}

fn push_wrapped(out: &mut Vec<Line<'static>>, text: &str, width: usize, style: Style) {
    let wrapped = wrap(text, width.saturating_sub(4).max(1));
    let total = wrapped.len();
    for w in wrapped.into_iter().take(OBJECTIVE_LINES) {
        out.push(Line::styled(format!("    {w}"), style));
    }
    if total > OBJECTIVE_LINES {
        out.push(Line::styled(
            format!("    ... {} more lines", total - OBJECTIVE_LINES),
            DIM,
        ));
    }
}

#[cfg(test)]
#[path = "pinned_tests.rs"]
mod tests;
