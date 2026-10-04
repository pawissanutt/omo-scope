use super::*;
use crate::diff::{DiffLine, Mark};

fn thinking(text: &str) -> Entry {
    Entry {
        kind: Kind::Thinking,
        title: "thinking".into(),
        text: text.into(),
        detail: String::new(),
        result: None,
        at: None,
        command: String::new(),
        diff: Vec::new(),
        preview: None,
        links: Vec::new(),
    }
}

fn text_of(lines: &[LogLine]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

#[test]
fn edits_render_as_diff_previews_that_expand() {
    let mut e = thinking("");
    e.kind = Kind::Tool;
    e.title = "write".into();
    e.text = "write a.txt  +10 -0".into();
    e.diff.push(DiffLine {
        mark: Mark::File,
        text: "write a.txt".into(),
    });
    e.diff.extend((0..10).map(|i| DiffLine {
        mark: Mark::Add,
        text: format!("l{i}"),
    }));
    let entries = vec![e];
    let closed = text_of(&render_entries(
        &entries,
        &HashSet::new(),
        60,
        Reasoning::Preview,
        None,
        None,
    ));
    assert_eq!(closed.len(), 8);
    assert_eq!(closed[1].trim(), "+ l0");
    assert!(closed[7].contains("4 more lines"));
    let open = render_entries(&entries, &HashSet::from([0]), 60, Reasoning::Preview, None, None);
    assert_eq!(open.len(), 11);
}

#[test]
fn reasoning_modes_control_thinking_output() {
    let entries = vec![thinking("**Plan**\na\nb\nc\nd")];
    let none = HashSet::new();
    assert!(render_entries(&entries, &none, 40, Reasoning::Hidden, None, None).is_empty());

    let preview = text_of(&render_entries(&entries, &none, 40, Reasoning::Preview, None, None));
    assert_eq!(preview[0], "  │ Plan");
    assert_eq!(preview.len(), 4);
    assert!(preview[3].contains("2 more lines"));

    let full = render_entries(&entries, &none, 40, Reasoning::Full, None, None);
    assert_eq!(full.len(), 5);
    assert!(full.iter().all(|l| l.entry == Some(0)));
}

#[test]
fn link_lines_follow_the_header_and_target_their_task() {
    let mut e = thinking("");
    e.kind = Kind::Tool;
    e.title = "task_send".into();
    e.text = "→ fix: go".into();
    e.result = Some(crate::transcript::ToolResult {
        text: "lane_capacity".into(),
        is_error: true,
    });
    e.preview = Some(String::new());
    e.links.push(Link {
        task: "st_2".into(),
        name: "fix".into(),
        note: "lane_capacity".into(),
        error: true,
    });
    let lines = render_entries(&[e], &HashSet::new(), 60, Reasoning::Preview, None, None);
    let text = text_of(&lines);
    assert_eq!(text, ["  ✗ task_send → fix: go", "      ⇢ ? fix — lane_capacity"]);
    assert_eq!((lines[0].entry, lines[0].task.as_deref()), (Some(0), None));
    assert_eq!((lines[1].entry, lines[1].task.as_deref()), (None, Some("st_2")));
}

#[test]
fn title_parts_wrap_onto_lines_that_fit() {
    let parts: Vec<String> = ["19 turns", "39 tok/s", "38k ctx", "92% cache"]
        .map(String::from)
        .into();
    assert_eq!(wrap_parts(&parts, 24), [" 19 turns · 39 tok/s", " 38k ctx · 92% cache"]);
    assert_eq!(wrap_parts(&parts, 80), [" 19 turns · 39 tok/s · 38k ctx · 92% cache"]);
    assert!(wrap_parts(&[], 80).is_empty());
}

#[test]
fn row_meta_drops_extras_then_trailing_stats_and_keeps_elapsed() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let fit = |room| fit_meta(s(&["deep", "gpt"]), s(&["18 turns", "$1.46"]), "9m50s".into(), room);
    assert_eq!(fit(80), "deep · gpt · 18 turns · $1.46 · 9m50s ");
    assert_eq!(fit(30), "18 turns · $1.46 · 9m50s ");
    assert_eq!(fit(20), "18 turns · 9m50s ");
    assert_eq!(fit(3), "9m50s ");
}
