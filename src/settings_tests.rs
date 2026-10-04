use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::stats::StatKey;

fn press(s: &mut Settings, cfg: &mut Config, model: Option<&str>, code: KeyCode) -> Option<Action> {
    match action(s.selected, code)? {
        Action::Op(op) => {
            apply(s, cfg, model, op);
            None
        }
        other => Some(other),
    }
}

#[test]
fn toggles_bar_and_row_for_selected_key() {
    let mut s = Settings::default();
    let mut cfg = Config::default();
    press(&mut s, &mut cfg, None, KeyCode::Char('j'));
    let key = cfg.order[1];
    let (bar, row) = (cfg.bar.contains(&key), cfg.row.contains(&key));
    press(&mut s, &mut cfg, None, KeyCode::Char(' '));
    press(&mut s, &mut cfg, None, KeyCode::Char('w'));
    assert_eq!(cfg.bar.contains(&key), !bar);
    assert_eq!(cfg.row.contains(&key), !row);
    press(&mut s, &mut cfg, None, KeyCode::Char('b'));
    assert_eq!(cfg.bar.contains(&key), bar);
}

#[test]
fn selection_is_clamped() {
    let mut s = Settings::default();
    let mut cfg = Config::default();
    press(&mut s, &mut cfg, None, KeyCode::Up);
    assert_eq!(s.selected, 0);
    for _ in 0..20 {
        press(&mut s, &mut cfg, None, KeyCode::Down);
    }
    assert_eq!(s.selected, cfg.order.len() - 1);
}

#[test]
fn reorder_keeps_selection_on_moved_key() {
    let mut s = Settings::default();
    let mut cfg = Config::default();
    press(&mut s, &mut cfg, None, KeyCode::Char('j'));
    let key = cfg.order[1];
    press(&mut s, &mut cfg, None, KeyCode::Char('J'));
    assert_eq!(s.selected, 2);
    assert_eq!(cfg.order[2], key);
    press(&mut s, &mut cfg, None, KeyCode::Char('K'));
    press(&mut s, &mut cfg, None, KeyCode::Char('K'));
    assert_eq!(s.selected, 0);
    assert_eq!(cfg.order[0], key);
    press(&mut s, &mut cfg, None, KeyCode::Char('K'));
    assert_eq!(s.selected, 0);
}

#[test]
fn m_cycles_cost_mode() {
    let mut s = Settings::default();
    let mut cfg = Config::default();
    let seen: Vec<CostMode> = (0..3)
        .map(|_| {
            press(&mut s, &mut cfg, None, KeyCode::Char('m'));
            cfg.cost
        })
        .collect();
    assert_eq!(seen, [CostMode::Always, CostMode::Never, CostMode::Auto]);
}

#[test]
fn ctx_limit_steps_for_model_only() {
    let mut s = Settings::default();
    let mut cfg = Config::default();
    cfg.auto_limits.insert("m1".into(), 200_000);
    press(&mut s, &mut cfg, Some("m1"), KeyCode::Char(']'));
    assert_eq!(cfg.ctx_limit("m1"), Some(256_000));
    assert!(!cfg.ctx_is_auto("m1"));
    press(&mut s, &mut cfg, Some("m1"), KeyCode::Char('['));
    assert_eq!(cfg.ctx_limit("m1"), Some(200_000));
    let before = cfg.clone();
    assert!(!apply(&mut s, &mut cfg, None, Op::CtxUp));
    assert_eq!(cfg, before);
}

#[test]
fn reset_restores_defaults_and_keeps_auto_limits() {
    let mut s = Settings::default();
    let mut cfg = Config::default();
    cfg.auto_limits.insert("m1".into(), 200_000);
    press(&mut s, &mut cfg, None, KeyCode::Char('J'));
    press(&mut s, &mut cfg, None, KeyCode::Char('w'));
    press(&mut s, &mut cfg, None, KeyCode::Char('m'));
    press(&mut s, &mut cfg, Some("m1"), KeyCode::Char('+'));
    s.status = Some("saved".into());
    press(&mut s, &mut cfg, None, KeyCode::Char('R'));
    let mut want = Config::default();
    want.auto_limits.insert("m1".into(), 200_000);
    assert_eq!(cfg, want);
    assert_eq!(s.status, None);
    assert!(cfg.bar.contains(&StatKey::Turns));
}

#[test]
fn close_and_save_keys() {
    for code in [KeyCode::Esc, KeyCode::Char('c'), KeyCode::Char('q')] {
        assert_eq!(action(0, code), Some(Action::Close));
    }
    for code in [KeyCode::Enter, KeyCode::Char('S')] {
        assert_eq!(action(0, code), Some(Action::Save));
    }
    assert_eq!(action(0, KeyCode::Char('z')), None);
}
