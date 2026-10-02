use serde_json::json;

use super::*;

#[test]
fn apply_patch_becomes_files_hunks_and_counts() {
    let input = "*** Begin Patch\n*** Update File: /w/src/a.rs\n*** Move to: /w/src/b.rs\n@@ fn main\n ctx\n-old\n+new\n+more\n\
                 *** Add File: /w/c.rs\n+x\n*** End Patch";
    let c = from_call("apply_patch", &json!({"input": input}), Some(Path::new("/w"))).unwrap();
    assert_eq!(c.summary, "2 files  +3 -1");
    assert_eq!(c.lines[0].text, "update src/a.rs -> src/b.rs");
    let marks: Vec<Mark> = c.lines.iter().map(|l| l.mark).collect();
    use Mark::*;
    assert_eq!(marks, [File, Hunk, Ctx, Del, Add, Add, File, Add]);
}

#[test]
fn write_and_edit_calls_become_diffs() {
    let w = from_call(
        "write",
        &json!({"path": "/w/n.txt", "content": "a\nb"}),
        Some(Path::new("/w")),
    )
    .unwrap();
    assert_eq!(w.summary, "write n.txt  +2 -0");
    let edits =
        json!({"path": "x.rs", "edits": [{"oldText": "a", "newText": "b\nc"}, {"oldText": "d", "newText": ""}]});
    let e = from_call("edit", &edits, None).unwrap();
    assert_eq!(e.summary, "edit x.rs  +2 -2");
    assert_eq!(e.lines.iter().filter(|l| l.mark == Mark::Hunk).count(), 1);
    assert!(from_call("read", &json!({"path": "x.rs"}), None).is_none());
    assert!(from_call("bash", &json!({"command": "ls"}), None).is_none());
}
