use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

const SAMPLE: &str = "\
bar = turns, tools, tps, ctx, cache, cost
row = cost
cost = auto
ctx-limit.gpt-6-astra = 400000
";

fn assert_permutation(order: &[StatKey]) {
    assert_eq!(order.len(), StatKey::ALL.len());
    for key in StatKey::ALL {
        assert_eq!(
            order.iter().filter(|item| **item == key).count(),
            1,
            "{key:?} missing from {order:?}"
        );
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("omo-scope-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn parse_sample() {
    let cfg = Config::parse(SAMPLE);
    assert_eq!(
        cfg.bar_keys(),
        vec![
            StatKey::Turns,
            StatKey::Tools,
            StatKey::Tps,
            StatKey::Ctx,
            StatKey::Cache,
            StatKey::Cost
        ]
    );
    assert_eq!(cfg.row_keys(), vec![StatKey::Cost]);
    assert_eq!(cfg.cost, CostMode::Auto);
    assert_eq!(cfg.ctx_limits.get("gpt-6-astra"), Some(&400_000));
    assert!(cfg.auto_limits.is_empty());
    assert_eq!(cfg.order, StatKey::ALL.to_vec());
    assert_permutation(&cfg.order);
}

#[test]
fn parse_ignores_junk() {
    let text = "\
# comment

not a pair
unknown = 1
bar = turns, nope, tools, turns
row =
cost = sometimes
ctx-limit.bad = no
ctx-limit. = 3
ctx-limit.kept = 42
";
    let cfg = Config::parse(text);
    assert_eq!(cfg.bar_keys(), vec![StatKey::Turns, StatKey::Tools]);
    assert!(cfg.row.is_empty());
    assert_eq!(cfg.cost, CostMode::Auto);
    assert_eq!(cfg.ctx_limits.get("kept"), Some(&42));
    assert!(!cfg.ctx_limits.contains_key("bad"));
    assert!(!cfg.ctx_limits.contains_key(""));
    assert_eq!(cfg.order[0], StatKey::Turns);
    assert_eq!(cfg.order[1], StatKey::Tools);
    assert_permutation(&cfg.order);
}

#[test]
fn parse_bar_none() {
    let cfg = Config::parse("bar = none\n");
    assert!(cfg.bar.is_empty());
    assert!(cfg.bar_keys().is_empty());
    assert_eq!(cfg.row, Config::default().row);
    assert_eq!(cfg.to_text().lines().nth(1), Some("bar = none"));
    assert_permutation(&cfg.order);
    let again = Config::parse(&cfg.to_text());
    assert_eq!(again, cfg);
}

#[test]
fn round_trip_ignores_auto_limits() {
    let mut cfg = Config::parse(SAMPLE);
    cfg.auto_limits.insert("catalog-only".into(), 999);
    let text = cfg.to_text();
    assert_eq!(
        text,
        "\
# omo-scope stats config
bar = turns, tools, tps, ctx, cache, cost
row = cost
cost = auto
ctx-limit.gpt-6-astra = 400000
"
    );
    let again = Config::parse(&text);
    assert!(again.auto_limits.is_empty());
    cfg.auto_limits.clear();
    assert_eq!(again, cfg);

    let custom = Config::parse(
        "bar = none\nrow = io, tok\ncost = never\nctx-limit.z-model = 1000000\nctx-limit.a-model = 128000\n",
    );
    let saved = custom.to_text();
    assert!(saved.contains("bar = none"));
    assert!(saved.contains("row = io, tok"));
    assert!(saved.contains("cost = never"));
    let a = saved.find("ctx-limit.a-model").unwrap();
    let z = saved.find("ctx-limit.z-model").unwrap();
    assert!(a < z);
    let mut round = custom.clone();
    round.auto_limits.insert("hidden".into(), 3);
    let parsed = Config::parse(&round.to_text());
    assert!(parsed.auto_limits.is_empty());
    assert_eq!(parsed, custom);
}

#[test]
fn set_bar_list_reorders_and_rejects_unknown() {
    let mut cfg = Config::default();
    cfg.set_bar_list("tok, io").unwrap();
    assert_eq!(cfg.bar_keys(), vec![StatKey::Tok, StatKey::Io]);
    assert_eq!(&cfg.order[..2], &[StatKey::Tok, StatKey::Io]);
    let rest: Vec<_> = StatKey::ALL
        .into_iter()
        .filter(|key| !matches!(key, StatKey::Tok | StatKey::Io))
        .collect();
    assert_eq!(&cfg.order[2..], rest.as_slice());
    assert_permutation(&cfg.order);

    cfg.toggle_bar(StatKey::Tok);
    assert!(!cfg.bar.contains(&StatKey::Tok));
    assert_eq!(cfg.order[0], StatKey::Tok);
    cfg.toggle_bar(StatKey::Tok);
    assert!(cfg.bar.contains(&StatKey::Tok));
    cfg.toggle_row(StatKey::Io);
    assert!(cfg.row.contains(&StatKey::Io));
    cfg.toggle_row(StatKey::Cost);
    assert!(!cfg.row.contains(&StatKey::Cost));

    cfg.set_bar_list("none").unwrap();
    assert!(cfg.bar.is_empty());
    cfg.set_bar_list("").unwrap();
    assert!(cfg.bar.is_empty());
    cfg.set_bar_list("tok, io").unwrap();

    let before = cfg.clone();
    let err = cfg.set_bar_list("turns, nope").unwrap_err().to_string();
    assert!(err.contains("nope"), "{err}");
    for key in StatKey::ALL {
        assert!(err.contains(key.name()), "{err}");
    }
    assert_eq!(cfg, before);
}

#[test]
fn move_key_stops_at_ends() {
    let mut cfg = Config::default();
    let original = cfg.order.clone();
    assert_eq!(cfg.move_key(0, true), 0);
    assert_eq!(cfg.move_key(cfg.order.len() - 1, false), cfg.order.len() - 1);
    assert_eq!(cfg.move_key(cfg.order.len(), true), cfg.order.len());
    assert_eq!(cfg.order, original);

    let second = cfg.order[1];
    assert_eq!(cfg.move_key(1, true), 0);
    assert_eq!(cfg.order[0], second);
    assert_eq!(cfg.order[1], original[0]);
    assert_eq!(cfg.move_key(0, false), 1);
    assert_eq!(cfg.order, original);
}

#[test]
fn step_ctx_limit_up_down_and_back_to_auto() {
    let mut cfg = Config::default();
    let model = "gpt-6-astra";
    assert!(cfg.ctx_is_auto(model));
    assert_eq!(cfg.ctx_limit(model), None);

    cfg.step_ctx_limit(model, true);
    assert_eq!(cfg.ctx_limit(model), Some(CTX_STEPS[0]));
    assert!(!cfg.ctx_is_auto(model));
    cfg.step_ctx_limit(model, true);
    assert_eq!(cfg.ctx_limit(model), Some(CTX_STEPS[1]));
    cfg.step_ctx_limit(model, false);
    assert_eq!(cfg.ctx_limit(model), Some(CTX_STEPS[0]));
    cfg.step_ctx_limit(model, false);
    assert!(cfg.ctx_is_auto(model));
    assert_eq!(cfg.ctx_limit(model), None);

    cfg.auto_limits.insert(model.into(), 256_000);
    assert!(cfg.ctx_is_auto(model));
    assert_eq!(cfg.ctx_limit(model), Some(256_000));
    cfg.step_ctx_limit(model, true);
    assert_eq!(cfg.ctx_limits.get(model).copied(), Some(400_000));
    assert!(!cfg.ctx_is_auto(model));

    cfg.ctx_limits.insert(model.into(), CTX_STEPS[CTX_STEPS.len() - 1]);
    cfg.step_ctx_limit(model, true);
    assert_eq!(cfg.ctx_limit(model), Some(CTX_STEPS[CTX_STEPS.len() - 1]));

    cfg.ctx_limits.insert(model.into(), 2_000_000);
    cfg.step_ctx_limit(model, true);
    assert_eq!(cfg.ctx_limit(model), Some(1_000_000));

    cfg.ctx_limits.insert(model.into(), 150_000);
    cfg.step_ctx_limit(model, false);
    assert_eq!(cfg.ctx_limit(model), Some(128_000));

    cfg.ctx_limits.insert(model.into(), 50_000);
    cfg.step_ctx_limit(model, false);
    assert!(cfg.ctx_is_auto(model));
    assert_eq!(cfg.ctx_limit(model), Some(256_000));

    cfg.cost = CostMode::Never;
    cfg.ctx_limits.insert(model.into(), 128_000);
    cfg.reset();
    assert_eq!(cfg.cost, CostMode::Auto);
    assert!(cfg.ctx_limits.is_empty());
    assert_eq!(cfg.order, StatKey::ALL.to_vec());
    assert_eq!(cfg.auto_limits.get(model), Some(&256_000));
    assert_eq!(cfg.ctx_limit(model), Some(256_000));
}

#[test]
fn limits_from_models_reads_both_shapes() {
    let catalog = json!({
        "providers": {
            "openai": {
                "models": [
                    {"id": "gpt-6.1-sol", "contextWindow": 1},
                    {"id": "gpt-6.1-sol", "contextWindow": 400000},
                    {"id": "no-window"},
                    {"contextWindow": 9},
                    {"id": 5, "contextWindow": 9},
                    {"id": "zero", "contextWindow": 0}
                ]
            }
        }
    });
    let limits = Config::limits_from_models(&catalog);
    assert_eq!(limits.get("gpt-6.1-sol"), Some(&400_000));
    assert_eq!(limits.get("zero"), Some(&0));
    assert!(!limits.contains_key("no-window"));
    assert_eq!(limits.len(), 2);

    let store = json!({
        "anthropic": {"models": [{"id": "claude-opus", "contextWindow": 200000}]},
        "other": {"not-models": [{"id": "skip", "contextWindow": 9}]}
    });
    let limits = Config::limits_from_models(&store);
    assert_eq!(limits.get("claude-opus"), Some(&200_000));
    assert!(!limits.contains_key("skip"));
    assert_eq!(limits.len(), 1);
}

#[test]
fn save_to_then_load_from() {
    let dir = TempDir::new();
    let path = dir.path().join("nested").join("config");
    let mut cfg = Config::parse(
        "bar = tok, io\nrow = turns\ncost = always\nctx-limit.gpt-6-astra = 400000\nctx-limit.aaa = 128000\n",
    );
    cfg.auto_limits.insert("not-saved".into(), 7);
    cfg.save_to(&path).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("not-saved"));
    assert!(text.find("ctx-limit.aaa").unwrap() < text.find("ctx-limit.gpt-6-astra").unwrap());
    assert!(!dir.path().join("nested").join("config.tmp").exists());

    let loaded = Config::load_from(&path);
    assert!(loaded.auto_limits.is_empty());
    cfg.auto_limits.clear();
    assert_eq!(loaded, cfg);

    assert_eq!(Config::load_from(&dir.path().join("missing")), Config::default());
    let not_a_file = dir.path().join("adir");
    fs::create_dir_all(&not_a_file).unwrap();
    assert_eq!(Config::load_from(&not_a_file), Config::default());

    let resolved = Config::path().expect("config path");
    assert!(resolved.ends_with(Path::new("omo-scope").join("config")));
    assert_eq!(Config::load().order.len(), StatKey::ALL.len());
}
