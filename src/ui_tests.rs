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
    let closed = text_of(&render_entries(&entries, &HashSet::new(), 60, Reasoning::Preview, None));
    assert_eq!(closed.len(), 8);
    assert_eq!(closed[1].trim(), "+ l0");
    assert!(closed[7].contains("4 more lines"));
    let open = render_entries(&entries, &HashSet::from([0]), 60, Reasoning::Preview, None);
    assert_eq!(open.len(), 11);
}

#[test]
fn reasoning_modes_control_thinking_output() {
    let entries = vec![thinking("**Plan**\na\nb\nc\nd")];
    let none = HashSet::new();
    assert!(render_entries(&entries, &none, 40, Reasoning::Hidden, None).is_empty());

    let preview = text_of(&render_entries(&entries, &none, 40, Reasoning::Preview, None));
    assert_eq!(preview[0], "  │ Plan");
    assert_eq!(preview.len(), 4);
    assert!(preview[3].contains("2 more lines"));

    let full = render_entries(&entries, &none, 40, Reasoning::Full, None);
    assert_eq!(full.len(), 5);
    assert!(full.iter().all(|l| l.entry == Some(0)));
}
