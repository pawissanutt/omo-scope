use super::*;

fn thinking(text: &str) -> Entry {
    Entry {
        kind: Kind::Thinking,
        title: "thinking".into(),
        text: text.into(),
        detail: String::new(),
        result: None,
        at: None,
        command: String::new(),
    }
}

fn text_of(lines: &[LogLine]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
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
