use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use serde_json::Value;

use crate::store::home;

const TITLE: &str = "omo-scope";

fn bin() -> PathBuf {
    std::env::var_os("HERDR_BIN_PATH")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("herdr"))
}

fn pane(args: &[&str]) -> anyhow::Result<Value> {
    let out = Command::new(bin())
        .arg("pane")
        .args(args)
        .output()
        .context("cannot run herdr")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let reply: Value = if stdout.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&stdout)?
    };
    if let Some(msg) = reply["error"]["message"].as_str() {
        bail!("herdr pane {}: {msg}", args[0]);
    }
    if !out.status.success() {
        bail!(
            "herdr pane {} failed: {}",
            args[0],
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(reply["result"].clone())
}

fn is_ours(pane_id: &str) -> bool {
    pane(&["get", pane_id]).is_ok_and(|r| r["pane"]["label"] == TITLE)
}

fn record_path(parent: &str) -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local").join("state"));
    let name: String = parent
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    base.join("omo-scope").join(format!("{name}.pane"))
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn open(root: &Path, session: Option<String>, stats: Option<String>, ratio: f32) -> anyhow::Result<()> {
    let parent = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .context("not inside a Herdr pane (HERDR_PANE_ID is unset); run `omo-scope` directly instead")?;
    let record = record_path(&parent);
    if let Ok(existing) = std::fs::read_to_string(&record) {
        let existing = existing.trim();
        if !existing.is_empty() && is_ours(existing) {
            println!("omo-scope is already open in pane {existing}");
            return Ok(());
        }
    }
    let root_s = root.to_string_lossy();
    let ratio_s = format!("{ratio:.2}");
    let split = pane(&[
        "split",
        "--pane",
        &parent,
        "--direction",
        "right",
        "--ratio",
        &ratio_s,
        "--no-focus",
        "--cwd",
        &root_s,
    ])?;
    let id = split["pane"]["pane_id"]
        .as_str()
        .context("herdr split returned no pane id")?
        .to_string();
    pane(&["rename", &id, TITLE])?;
    let exe = std::env::current_exe()?;
    let mut cmd = vec![
        quote(&exe.to_string_lossy()),
        "--cwd".into(),
        quote(&root_s),
        "--close-pane".into(),
    ];
    if let Some(s) = session
        .or_else(|| std::env::var("PI_SESSION_ID").ok())
        .filter(|s| !s.is_empty())
    {
        cmd.extend(["--session".into(), quote(&s)]);
    }
    if let Some(list) = stats {
        cmd.extend(["--stats".into(), quote(&list)]);
    }
    pane(&["run", &id, &cmd.join(" ")])?;
    std::fs::create_dir_all(record.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(&record, &id)?;
    println!("omo-scope opened in pane {id}");
    Ok(())
}

pub fn close_own_pane() {
    if let Ok(id) = std::env::var("HERDR_PANE_ID")
        && let Err(e) = pane(&["close", &id])
    {
        eprintln!("omo-scope: could not close pane {id}: {e}");
    }
}
