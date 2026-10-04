use serde_json::json;

use super::*;

fn plan() -> Plan {
    Plan::from_state(&json!({"schema": "v2", "phases": [
        {"name": "Survey", "tasks": [
            {"content": "Read code", "status": "completed"},
            {"content": "Drop me", "status": "abandoned"}]},
        {"name": "Build", "tasks": [
            {"content": "Write it", "status": "pending"},
            {"content": "Test it", "status": "pending"}]},
        {"name": "Empty", "tasks": []}]}))
    .unwrap()
}

fn text(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

#[test]
fn plan_counts_closed_steps_and_finds_the_current_one() {
    let p = plan();
    assert_eq!(p.phases.len(), 2);
    assert_eq!(p.progress(), (2, 4));
    let (phase, step) = p.current().unwrap();
    assert_eq!((phase.name.as_str(), step.content.as_str()), ("Build", "Write it"));
    assert!(Plan::from_state(&json!({"phases": []})).is_none());
}

#[test]
fn collapsed_strip_is_one_line_each_and_open_strip_respects_max() {
    let goal = Goal::from_json(&json!({"objective": "Ship it", "status": "blocked",
        "tokensUsed": 93544, "timeUsedSeconds": 9860, "blockedReason": "waiting on user"}))
    .unwrap();
    let p = plan();
    let closed = text(&lines(Some(&p), Some(&goal), false, 80, 2));
    assert_eq!(
        closed,
        [
            "▸ ◎ goal blocked · 94k tok · 2h44m · waiting on user",
            "▸ plan 2/4 · Build ▸ ○ Write it"
        ]
    );
    let open = text(&lines(Some(&p), None, true, 80, 20));
    assert_eq!(
        open,
        [
            "▾ plan 2/4",
            "  ✓ Survey 2/2",
            "  Build 0/2",
            "    ○ Write it",
            "    ○ Test it"
        ]
    );
    let capped = text(&lines(Some(&p), None, true, 80, 3));
    assert_eq!(capped.len(), 3);
    assert!(capped[2].contains("3 more lines"));
}
