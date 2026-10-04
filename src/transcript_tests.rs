use std::io::Write;

use serde_json::json;

use super::*;

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("omo-scope-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("t.jsonl")
}

#[test]
fn tail_buffers_partial_lines_and_detects_truncation() {
    let path = temp_file("tail");
    std::fs::write(&path, "{\"a\":1}\n{\"b\":").unwrap();
    let mut tail = Tail::new(path.clone());
    let b = tail.poll().unwrap();
    assert_eq!(b.values.len(), 1);
    assert!(!b.reset);

    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    f.write_all(b"2}\nnot json\n").unwrap();
    let b = tail.poll().unwrap();
    assert_eq!(b.values, vec![json!({"b": 2})]);
    assert_eq!(b.bad, 1);

    std::fs::write(&path, "{\"c\":3}\n").unwrap();
    let b = tail.poll().unwrap();
    assert!(b.reset);
    assert_eq!(b.values, vec![json!({"c": 3})]);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn tool_results_attach_to_their_calls() {
    let mut t = Transcript::default();
    t.push(
        &json!({"type": "message", "timestamp": "2026-10-02T07:35:43.373Z", "message": {
        "role": "assistant", "stopReason": "toolUse", "content": [
            {"type": "thinking", "thinking": "plan"},
            {"type": "text", "text": "Running it."},
            {"type": "toolCall", "id": "c1", "name": "bash", "arguments": {"command": "ls -la\nmore"}}
        ]}}),
    );
    assert_eq!(t.running_tool().map(|e| e.title.as_str()), Some("bash"));
    t.push(&json!({"type": "message", "message": {
        "role": "toolResult", "toolCallId": "c1", "toolName": "bash", "isError": false,
        "content": [{"type": "text", "text": "ok"}]}}));
    assert!(t.running_tool().is_none());
    let kinds: Vec<Kind> = t.entries.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [Kind::Thinking, Kind::Assistant, Kind::Tool]);
    assert_eq!(t.entries[2].text, "ls -la");
    assert_eq!(t.entries[2].result.as_ref().unwrap().text, "ok");
}

#[test]
fn abandoned_calls_are_closed_by_the_next_turn() {
    let mut t = Transcript::default();
    let call = json!({"type": "message", "message": {"role": "assistant", "content": [
        {"type": "toolCall", "id": "c1", "name": "read", "arguments": {"path": "a.rs"}}]}});
    t.push(&call);
    t.push(
        &json!({"type": "message", "message": {"role": "assistant", "stopReason": "error",
        "errorMessage": "boom", "content": []}}),
    );
    assert!(t.running_tool().is_none());
    assert!(t.entries[0].result.as_ref().unwrap().is_error);
    assert_eq!(t.entries[1].kind, Kind::Error);
}

#[test]
fn eval_cells_expose_embedded_shell_commands() {
    let code = "const a = await tool.bash({ command: \"make > /tmp/a.log\", timeout: 9 });\n\
                await tool.bash({command:`cd /w && x | tee b.log`});";
    let mut t = Transcript::default();
    t.push(&json!({"type": "message", "message": {"role": "assistant", "content": [
        {"type": "toolCall", "id": "c1", "name": "eval", "arguments": {"code": code}}]}}));
    assert_eq!(t.entries[0].command, "make > /tmp/a.log\ncd /w && x | tee b.log");
}

#[test]
fn push_feeds_usage_from_assistant_and_compaction_lines() {
    let mut t = Transcript::default();
    t.push(&json!({
        "type": "message",
        "timestamp": "1970-01-01T00:00:01Z",
        "message": {
            "role": "assistant",
            "provider": "chatgpt-subscription",
            "timestamp": 0,
            "content": [{"type": "toolCall", "id": "c1", "name": "bash", "arguments": {"command": "true"}}],
            "usage": {
                "input": 10,
                "output": 4,
                "cacheRead": 6,
                "cacheWrite": 0,
                "reasoning": 0,
                "totalTokens": 20,
                "cost": {"total": 0.1}
            }
        }
    }));
    t.push(&json!({"type": "compaction", "tokensBefore": 100, "summary": "s"}));
    let s = t.usage.stats();
    assert_eq!(s.turns, Some(1));
    assert_eq!(s.tool_calls, Some(1));
    assert_eq!(s.total, Some(20));
    assert_eq!(s.compactions, Some(1));
    assert!(s.subscription);
    assert_eq!(t.entries.iter().filter(|e| e.kind == Kind::Tool).count(), 1);
}

#[test]
fn todo_state_records_set_and_clear_the_plan() {
    let mut t = Transcript::default();
    t.push(
        &json!({"type": "custom", "customType": "senpi.todo-state", "data": {"phases": [
        {"name": "A", "tasks": [{"content": "x", "status": "in_progress"}]}]}}),
    );
    assert_eq!(t.plan.as_ref().map(|p| p.progress()), Some((0, 1)));
    t.push(&json!({"type": "custom", "customType": "senpi.todo-state", "data": {"phases": []}}));
    assert!(t.plan.is_none());
    assert!(t.entries.is_empty());
}

#[test]
fn goal_results_track_the_goal_and_count_wakes() {
    let mut t = Transcript::default();
    let goal = |id: &str, name: &str, status: &str| {
        json!({"type": "message", "message": {"role": "toolResult", "toolCallId": id, "toolName": name,
            "isError": false, "content": [{"type": "text", "text": "{}"}],
            "details": {"goal": {"objective": "Ship", "status": status, "tokensUsed": 1200}}}})
    };
    t.push(&goal("c1", "create_goal", "active"));
    let wake = json!({"type": "custom_message", "customType": "goal-continuation", "content": "go", "display": false});
    t.push(&wake);
    t.push(&wake);
    t.push(&goal("c2", "update_goal", "complete"));
    let g = t.goal.as_ref().unwrap();
    assert_eq!((g.status.as_str(), g.wakes), ("complete", 2));
    assert_eq!(t.entries[1].preview.as_deref(), Some("complete · 1.2k tok"));
    t.push(
        &json!({"type": "message", "message": {"role": "toolResult", "toolCallId": "c3",
        "toolName": "get_goal", "isError": false, "content": [{"type": "text", "text": "{\"goal\": null}"}]}}),
    );
    assert!(t.goal.is_none());
}

#[test]
fn hidden_custom_entries_are_skipped() {
    let mut t = Transcript::default();
    t.push(&json!({"type": "custom_message", "customType": "x", "content": "hidden", "display": false}));
    t.push(&json!({"type": "custom", "customType": "y", "data": {}}));
    assert!(t.entries.is_empty());
}
