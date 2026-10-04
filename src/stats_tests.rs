use serde_json::{Value, json};

use super::*;

fn example() -> Value {
    json!({
        "runtime_ms": 808408,
        "turns": 74,
        "tool_calls": 85,
        "output_tokens": 16058,
        "input_tokens": 312289,
        "cache_read_tokens": 21080960,
        "total_tokens": 21409307,
        "generation_ms": 683851,
        "tokens_per_second": 23,
        "cost_usd": 9.2632124,
        "cache_hit_rate_last": 0.997,
        "cache_hit_rate_run": 0.985,
        "token_status": "complete",
        "cost_status": "reported"
    })
}

fn populated() -> Stats {
    Stats {
        turns: Some(74),
        tool_calls: Some(85),
        input: Some(312_289),
        output: Some(16_058),
        cache_read: Some(21_080_960),
        cache_write: Some(0),
        reasoning: Some(2_100),
        total: Some(21_409_307),
        tps: Some(23.4),
        cost: Some(9.2632124),
        cache_rate: Some(0.985),
        context: Some(2_509_092),
        compactions: Some(3),
        subscription: true,
        partial: false,
    }
}

#[test]
fn from_run_stats_maps_the_task_file_object() {
    let v = example();
    let s = Stats::from_run_stats(&v, "chatgpt-subscription");
    assert_eq!(s.turns, Some(74));
    assert_eq!(s.tool_calls, Some(85));
    assert_eq!(s.input, Some(312_289));
    assert_eq!(s.output, Some(16_058));
    assert_eq!(s.cache_read, Some(21_080_960));
    assert_eq!(s.cache_write, None);
    assert_eq!(s.total, Some(21_409_307));
    assert_eq!(s.tps, Some(23.0));
    assert_eq!(s.cost, v["cost_usd"].as_f64());
    assert_eq!(s.cache_rate, v["cache_hit_rate_run"].as_f64());
    assert_eq!(s.reasoning, None);
    assert_eq!(s.context, None);
    assert_eq!(s.compactions, None);
    assert!(s.subscription);
    assert!(!s.partial);
    assert_eq!(s.render(StatKey::Cost, CostMode::Auto, None).as_deref(), Some("~$9.26"));
    assert_eq!(s.render(StatKey::Tok, CostMode::Auto, None).as_deref(), Some("21M tok"));
    assert_eq!(
        s.render(StatKey::Tps, CostMode::Auto, None).as_deref(),
        Some("23 tok/s")
    );

    let plain = Stats::from_run_stats(&v, "openai");
    assert!(!plain.subscription);
    assert_eq!(
        plain.render(StatKey::Cost, CostMode::Auto, None).as_deref(),
        Some("$9.26")
    );

    let missing = Stats::from_run_stats(&Value::Null, "chatgpt-subscription");
    assert_eq!(missing.turns, None);
    assert_eq!(missing.cost, None);
    assert!(missing.subscription);
    assert!(!missing.partial);
}

#[test]
fn partial_when_token_or_cost_status_is_unfinished() {
    let mut v = example();
    v["token_status"] = json!("estimating");
    assert!(Stats::from_run_stats(&v, "openai").partial);

    v["token_status"] = json!("complete");
    v["cost_status"] = json!("pending");
    assert!(Stats::from_run_stats(&v, "openai").partial);

    v["cost_status"] = json!("reported");
    assert!(!Stats::from_run_stats(&v, "openai").partial);
    assert!(!Stats::from_run_stats(&json!({"turns": 1}), "openai").partial);
}

#[test]
fn merge_keeps_recorded_values_and_fills_gaps_from_live() {
    let recorded = Stats {
        turns: Some(1),
        cost: None,
        context: Some(10),
        subscription: false,
        partial: true,
        ..Stats::default()
    };
    let live = Stats {
        turns: Some(9),
        cost: Some(1.5),
        context: None,
        compactions: Some(2),
        subscription: true,
        partial: false,
        ..Stats::default()
    };
    let merged = recorded.merge(&live);
    assert_eq!(merged.turns, Some(1));
    assert_eq!(merged.cost, Some(1.5));
    assert_eq!(merged.context, Some(10));
    assert_eq!(merged.compactions, Some(2));
    assert_eq!(merged.input, None);
    assert!(merged.subscription);
    assert!(merged.partial);
}

#[test]
fn render_formats_each_stat_and_hides_missing_data() {
    let full = populated();
    assert_eq!(
        full.render(StatKey::Turns, CostMode::Auto, None).as_deref(),
        Some("74 turns")
    );
    assert_eq!(
        full.render(StatKey::Tools, CostMode::Auto, None).as_deref(),
        Some("85 tools")
    );
    assert_eq!(
        full.render(StatKey::Tok, CostMode::Auto, None).as_deref(),
        Some("21M tok")
    );
    assert_eq!(
        full.render(StatKey::Io, CostMode::Auto, None).unwrap(),
        format!("312k in {} 16k out", '\u{00b7}')
    );
    assert_eq!(
        full.render(StatKey::Reasoning, CostMode::Auto, None).as_deref(),
        Some("2.1k think")
    );
    assert_eq!(
        full.render(StatKey::Tps, CostMode::Auto, None).as_deref(),
        Some("23 tok/s")
    );
    assert_eq!(
        full.render(StatKey::Cache, CostMode::Auto, None).as_deref(),
        Some("99% cache")
    );
    assert_eq!(
        full.render(StatKey::Ctx, CostMode::Auto, None).as_deref(),
        Some("2.5M ctx")
    );
    assert_eq!(
        full.render(StatKey::Ctx, CostMode::Auto, Some(10_000_000)).as_deref(),
        Some("2.5M/10M ctx 25%")
    );
    assert_eq!(
        full.render(StatKey::Ctx, CostMode::Auto, Some(0)).as_deref(),
        Some("2.5M ctx")
    );
    assert_eq!(
        full.render(StatKey::Compact, CostMode::Auto, None).unwrap(),
        format!("{}3", '\u{21e3}')
    );
    assert_eq!(
        full.render(StatKey::Cost, CostMode::Auto, None).as_deref(),
        Some("~$9.26")
    );
    assert_eq!(
        full.render(StatKey::Cost, CostMode::Always, None).as_deref(),
        Some("$9.26")
    );
    assert_eq!(full.render(StatKey::Cost, CostMode::Never, None), None);

    let mut plain = full.clone();
    plain.subscription = false;
    assert_eq!(
        plain.render(StatKey::Cost, CostMode::Never, None).as_deref(),
        Some("$9.26")
    );
    assert_eq!(
        plain.render(StatKey::Cost, CostMode::Auto, None).as_deref(),
        Some("$9.26")
    );

    let mut partial = full.clone();
    partial.partial = true;
    assert_eq!(
        partial.render(StatKey::Tok, CostMode::Auto, None).as_deref(),
        Some("21M tok?")
    );
    assert_eq!(
        partial.render(StatKey::Cost, CostMode::Auto, None).as_deref(),
        Some("~$9.26?")
    );
    assert_eq!(
        partial.render(StatKey::Turns, CostMode::Auto, None).as_deref(),
        Some("74 turns")
    );

    let mut half_up = full.clone();
    half_up.tps = Some(23.5);
    assert_eq!(
        half_up.render(StatKey::Tps, CostMode::Auto, None).as_deref(),
        Some("24 tok/s")
    );

    let zeros = Stats {
        reasoning: Some(0),
        compactions: Some(0),
        input: Some(10),
        ..Stats::default()
    };
    assert_eq!(zeros.render(StatKey::Reasoning, CostMode::Auto, None), None);
    assert_eq!(zeros.render(StatKey::Compact, CostMode::Auto, None), None);
    assert_eq!(zeros.render(StatKey::Io, CostMode::Auto, None), None);
    assert_eq!(zeros.render(StatKey::Cost, CostMode::Always, None), None);

    for key in StatKey::ALL {
        assert_eq!(Stats::default().render(key, CostMode::Always, Some(1_000)), None);
    }
}

#[test]
fn fmt_count_rounds_at_the_boundaries() {
    let cases = [
        (0, "0"),
        (999, "999"),
        (1_000, "1.0k"),
        (1_049, "1.0k"),
        (1_050, "1.1k"),
        (2_100, "2.1k"),
        (9_999, "10.0k"),
        (10_000, "10k"),
        (312_289, "312k"),
        (999_499, "999k"),
        (999_500, "1000k"),
        (1_000_000, "1.0M"),
        (2_509_092, "2.5M"),
        (9_949_999, "9.9M"),
        (9_950_000, "10.0M"),
        (10_000_000, "10M"),
        (21_409_307, "21M"),
    ];
    for (n, want) in cases {
        assert_eq!(Stats::fmt_count(n), want, "{n}");
    }
}

#[test]
fn usage_accumulates_two_assistant_turns_and_one_compaction() {
    assert_eq!(Usage::default().stats(), Stats::default());

    let mut usage = Usage::default();
    usage.push(&json!({"type": "message", "message": {"role": "user", "content": "hi"}}));
    usage.push(&json!({
        "type": "message",
        "message": {"role": "assistant", "content": [{"type": "toolCall"}]}
    }));
    usage.push(&json!({
        "type": "message",
        "timestamp": "1970-01-01T00:00:01Z",
        "message": {
            "role": "assistant",
            "provider": "chatgpt-subscription",
            "timestamp": 0,
            "content": [{"type": "toolCall"}, {"type": "text"}, {"type": "toolCall"}],
            "usage": {
                "input": 100,
                "output": 40,
                "cacheRead": 300,
                "cacheWrite": 0,
                "reasoning": 10,
                "totalTokens": 450,
                "cost": {"total": 1.25}
            }
        }
    }));
    usage.push(&json!({"type": "compaction", "tokensBefore": 378658}));
    usage.push(&json!({
        "type": "message",
        "timestamp": "1970-01-01T00:00:06Z",
        "message": {
            "role": "assistant",
            "provider": "openai",
            "timestamp": 5000,
            "content": [{"type": "toolCall"}],
            "usage": {
                "input": 200,
                "output": 60,
                "cacheRead": 100,
                "cacheWrite": 100,
                "reasoning": 5,
                "totalTokens": 465,
                "cost": {"total": 0.75}
            }
        }
    }));

    let s = usage.stats();
    assert_eq!(s.turns, Some(2));
    assert_eq!(s.tool_calls, Some(3));
    assert_eq!(s.input, Some(300));
    assert_eq!(s.output, Some(100));
    assert_eq!(s.cache_read, Some(400));
    assert_eq!(s.cache_write, Some(100));
    assert_eq!(s.reasoning, Some(15));
    assert_eq!(s.total, Some(915));
    assert_eq!(s.cost, Some(2.0));
    assert_eq!(s.tps, Some(50.0));
    assert_eq!(s.cache_rate, Some(0.5));
    assert_eq!(s.context, Some(400));
    assert_eq!(s.compactions, Some(1));
    assert!(s.subscription);
    assert!(!s.partial);
}
