use serde_json::json;

use super::*;

fn t(cmd: &str) -> Vec<String> {
    targets(cmd, Path::new("/r"))
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn targets_follow_redirects_tee_and_cd() {
    assert_eq!(t("cd /w && cargo test > /tmp/t.log 2>&1"), ["/tmp/t.log"]);
    assert_eq!(t("make 2>&1 | tee -a build.log"), ["/r/build.log"]);
    assert_eq!(t("cd sub; tail -f out.log"), ["/r/sub/out.log"]);
    assert_eq!(t("cargo build >> 'my dir/b.out'"), ["/r/my dir/b.out"]);
    assert!(t("echo hi >&2; ls > /dev/null; x &> \"$LOG\"").is_empty());
}

#[test]
fn live_tails_the_running_call_and_stops_on_result() {
    let dir = std::env::temp_dir().join(format!("omo-scope-live-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let log = dir.join("o.log");
    let mut tr = Transcript::default();
    tr.push(&json!({"type": "message", "message": {"role": "assistant", "content": [
        {"type": "toolCall", "id": "c1", "name": "bash",
         "arguments": {"command": format!("sleep 9 > {}", log.display())}}]}}));
    let mut live = None;
    assert!(!sync(&mut live, &tr, &dir));
    assert!(live.is_some());

    fs::write(&log, "a\n50%\r100%\nlast\n").unwrap();
    assert!(sync(&mut live, &tr, &dir));
    assert_eq!(live.as_ref().unwrap().lines, ["a", "100%", "last"]);
    assert!(!sync(&mut live, &tr, &dir));

    tr.push(
        &json!({"type": "message", "message": {"role": "toolResult", "toolCallId": "c1",
        "toolName": "bash", "isError": false, "content": []}}),
    );
    assert!(sync(&mut live, &tr, &dir));
    assert!(live.is_none());
    fs::remove_dir_all(&dir).unwrap();
}
