use serde_json::json;

use crate::transcript::Transcript;

fn call(t: &mut Transcript, id: &str, name: &str, args: serde_json::Value) {
    t.push(&json!({"type": "message", "message": {"role": "assistant", "content": [
        {"type": "toolCall", "id": id, "name": name, "arguments": args}]}}));
}

fn result(t: &mut Transcript, id: &str, name: &str, text: &str, details: serde_json::Value) {
    t.push(
        &json!({"type": "message", "message": {"role": "toolResult", "toolCallId": id,
        "toolName": name, "isError": false, "content": [{"type": "text", "text": text}], "details": details}}),
    );
}

#[test]
fn todo_calls_compact_and_hide_their_result() {
    let mut t = Transcript::default();
    call(
        &mut t,
        "c1",
        "todo",
        json!({"op": "init", "list": [
        {"phase": "A", "items": ["x", "y"]}, {"phase": "B", "items": ["z"]}]}),
    );
    result(&mut t, "c1", "todo", "Ask: ...\nNow: x", json!({}));
    call(&mut t, "c2", "todo", json!({"op": "done", "task": "x"}));
    assert_eq!(t.entries[0].text, "new plan · 3 tasks in 2 phases");
    assert_eq!(t.entries[0].preview.as_deref(), Some(""));
    assert_eq!(t.entries[1].text, "done · x");
}

#[test]
fn subagent_calls_link_to_task_ids_and_resolve_names() {
    let mut t = Transcript::default();
    call(
        &mut t,
        "c1",
        "task",
        json!({"tasks": [{"name": "rev", "task_summary": "Review"}, {"name": "fix"}]}),
    );
    result(
        &mut t,
        "c1",
        "task",
        "Batch running.",
        json!({"task_id": "st_1", "items": [
        {"task_id": "st_1", "name": "rev"}, {"task_id": "st_2", "name": "fix"}]}),
    );
    assert_eq!(t.entries[0].text, "spawn 2 · rev, fix");
    let links: Vec<(&str, &str)> = t.entries[0]
        .links
        .iter()
        .map(|l| (l.task.as_str(), l.name.as_str()))
        .collect();
    assert_eq!(links, [("st_1", "rev"), ("st_2", "fix")]);

    call(
        &mut t,
        "c2",
        "task_send",
        json!({"to": "fix", "message": "Finish now\nplease"}),
    );
    result(
        &mut t,
        "c2",
        "task_send",
        "Revived st_2 (run epoch 1).",
        json!({"kind": "revived"}),
    );
    let e = &t.entries[1];
    assert_eq!(e.text, "→ fix: Finish now");
    assert_eq!(e.links[0].task, "st_2");
    assert_eq!(e.links[0].note, "Revived st_2 (run epoch 1).");

    call(&mut t, "c3", "task_output", json!({"task_id": "st_1", "mode": "tail"}));
    result(
        &mut t,
        "c3",
        "task_output",
        "st_1 [running]",
        json!({"snapshot": {"task_id": "st_1"}}),
    );
    let e = &t.entries[2];
    assert_eq!(e.text, "peek st_1 (tail)");
    assert_eq!((e.links[0].name.as_str(), e.links[0].note.as_str()), ("rev", ""));
}

#[test]
fn eval_shows_language_summary_code_and_nested_calls() {
    let mut t = Transcript::default();
    call(
        &mut t,
        "c1",
        "eval",
        json!({"language": "js", "summary": "Survey disk", "code": "await x()"}),
    );
    result(
        &mut t,
        "c1",
        "eval",
        "{\n  \"out\": \"CLEAN_DONE\"",
        json!({"durationMs": 1500, "toolCalls": [
        {"name": "bash", "ok": true, "durationMs": 334, "args": {"command": "df -h"}},
        {"name": "bash", "ok": false, "durationMs": 5, "args": {"command": "false"}},
        {"name": "grep", "ok": true, "args": {"pattern": "fn"}}]}),
    );
    let e = &t.entries[0];
    assert_eq!(e.text, "[js] Survey disk");
    assert_eq!(
        e.preview.as_deref(),
        Some("1.5s · bash ×2 ✗, grep — \"out\": \"CLEAN_DONE\"")
    );
    assert_eq!(
        e.detail,
        "await x()\n── calls\n✓ bash 334ms  df -h\n✗ bash 5ms  false\n✓ grep   fn"
    );

    call(
        &mut t,
        "c2",
        "eval",
        json!({"language": "py", "summary": "Long", "code": "x"}),
    );
    result(
        &mut t,
        "c2",
        "eval",
        "Eval cell c2 detached and is running in the py kernel.",
        json!({"durationMs": 30000}),
    );
    assert_eq!(t.entries[1].preview.as_deref(), Some("⧗ detached · 30.0s"));
}

#[test]
fn read_and_memory_calls_become_one_liners() {
    let mut t = Transcript::default();
    t.push(&json!({"type": "session", "cwd": "/w"}));
    call(
        &mut t,
        "c1",
        "read",
        json!({"file_path": "/w/src/ui.rs", "offset": 462, "limit": 110}),
    );
    result(&mut t, "c1", "read", "a\nb\nc", json!(null));
    call(
        &mut t,
        "c2",
        "memory",
        json!({"command": "create", "file_path": "notes/a.md", "reason": "Record it"}),
    );
    assert_eq!(t.entries[0].text, "src/ui.rs:462-571");
    assert_eq!(t.entries[0].preview.as_deref(), Some("3 lines"));
    assert_eq!(t.entries[1].text, "create notes/a.md — Record it");
}
