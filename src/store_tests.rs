use serde_json::json;

use super::*;

fn put(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn store_builds_nested_tree_and_dag_levels() {
    let dir = std::env::temp_dir().join(format!("omo-scope-store-{}", std::process::id()));
    let task = |id: &str, parent: &str, created: &str| {
        json!({"task_id": id, "status": "running", "parent_session_id": parent,
            "root_session_id": "S", "created_at": created, "task_summary": format!("do {id}")})
        .to_string()
    };
    put(&dir.join("tasks/st_a.json"), &task("st_a", "S", "2026-10-02T00:00:00Z"));
    put(
        &dir.join("tasks/st_b.json"),
        &task("st_b", "CS", "2026-10-02T00:01:00Z"),
    );
    put(&dir.join("tasks/st_c.json"), &task("st_c", "S", "2026-10-02T00:02:00Z"));
    put(&dir.join("tasks/broken.json"), "{");
    put(
        &dir.join("children/st_a/sessions/st_a/2026_x.jsonl"),
        "{\"type\":\"session\",\"version\":3,\"id\":\"CS\"}\n",
    );
    let run = json!({"runId": "dag_1", "name": "r", "status": "running", "rootSessionId": "S",
        "nodes": [{"id": "late", "state": "pending", "dependsOn": ["early"]},
                  {"id": "early", "state": "completed", "dependsOn": []}]});
    put(&dir.join("dag/runs/dag_1.json"), &run.to_string());

    let mut store = Store::new(dir.clone());
    assert!(store.refresh());
    let tree: Vec<(usize, &str)> = store
        .session_tree("S")
        .into_iter()
        .map(|(d, t)| (d, t.id.as_str()))
        .collect();
    assert_eq!(tree, [(0, "st_a"), (1, "st_b"), (0, "st_c")]);
    assert_eq!(store.invalid(), 1);

    let runs = store.runs_for("S");
    let order: Vec<(&str, usize)> = runs[0].nodes.iter().map(|n| (n.id.as_str(), n.level)).collect();
    assert_eq!(order, [("early", 0), ("late", 1)]);
    assert_eq!(runs[0].progress(), (1, 2));

    assert!(!store.refresh());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cwd_encoding_matches_senpi_layout() {
    assert_eq!(
        encode_cwd(Path::new("/home/x/miti-workspace")),
        "--home-x-miti-workspace--"
    );
}

#[test]
fn task_reads_provider_and_run_stats() {
    let v = json!({
        "task_id": "st_stats",
        "status": "completed",
        "resolved_model": {"provider": "chatgpt-subscription", "model_id": "gpt-5"},
        "requested_model": {"provider": "ignored"},
        "model": "other/also-ignored",
        "run_stats": {
            "turns": 74,
            "tool_calls": 85,
            "output_tokens": 16058,
            "input_tokens": 312289,
            "cache_read_tokens": 21080960,
            "total_tokens": 21409307,
            "tokens_per_second": 23,
            "cost_usd": 9.2632124,
            "cache_hit_rate_run": 0.985,
            "token_status": "complete",
            "cost_status": "reported"
        }
    });
    let t = Task::from_json(&v).unwrap();
    assert_eq!(provider_of(&v), "chatgpt-subscription");
    assert_eq!(t.model, "gpt-5");
    assert_eq!(t.stats.total, Some(21_409_307));
    assert_eq!(t.stats.turns, Some(74));
    assert_eq!(t.stats.input, Some(312_289));
    assert_eq!(t.stats.output, Some(16_058));
    assert_eq!(t.stats.cache_read, Some(21_080_960));
    assert_eq!(t.stats.total, Some(21_409_307));
    assert_eq!(t.stats.tps, Some(23.0));
    assert_eq!(t.stats.cost, v["run_stats"]["cost_usd"].as_f64());
    assert_eq!(t.stats.cache_rate, v["run_stats"]["cache_hit_rate_run"].as_f64());
    assert!(t.stats.subscription);
    assert!(!t.stats.partial);
    assert_eq!(t.stats.reasoning, None);
    assert_eq!(t.stats.context, None);
    assert_eq!(t.stats.compactions, None);

    let requested = json!({
        "task_id": "st_req",
        "requested_model": {"provider": "anthropic-subscription"},
        "model": "openai/gpt-5"
    });
    let t = Task::from_json(&requested).unwrap();
    assert_eq!(provider_of(&requested), "anthropic-subscription");
    assert!(t.stats.subscription);

    let prefixed = json!({"task_id": "st_fb", "model": "openai/gpt-5", "run_stats": {"token_status": "partial"}});
    let t = Task::from_json(&prefixed).unwrap();
    assert_eq!(provider_of(&prefixed), "openai");
    assert!(t.stats.partial);
    assert!(!t.stats.subscription);

    let bare = json!({"task_id": "st_bare", "model": "gpt-5"});
    let t = Task::from_json(&bare).unwrap();
    assert_eq!(provider_of(&bare), "");
    assert_eq!(t.stats, crate::stats::Stats::default());
}

#[test]
fn user_title_skips_injected_tag_blocks() {
    let content = json!([{"type": "text", "text": "<system-reminder>\nnoise\n</system-reminder>\n\nFix the bug"}]);
    assert_eq!(user_title(&content), "Fix the bug");
}
