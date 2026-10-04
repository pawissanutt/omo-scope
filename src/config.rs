use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::Value;

use crate::stats::{CostMode, StatKey};

pub const CTX_STEPS: [u64; 5] = [128_000, 200_000, 256_000, 400_000, 1_000_000];

const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;

const DEFAULT_BAR: [StatKey; 6] = [
    StatKey::Turns,
    StatKey::Tools,
    StatKey::Tps,
    StatKey::Ctx,
    StatKey::Cache,
    StatKey::Cost,
];

const DEFAULT_ROW: [StatKey; 1] = [StatKey::Cost];

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Always a permutation of [`StatKey::ALL`].
    pub order: Vec<StatKey>,
    pub bar: HashSet<StatKey>,
    pub row: HashSet<StatKey>,
    pub cost: CostMode,
    /// User overrides keyed by model id. Saved.
    pub ctx_limits: BTreeMap<String, u64>,
    /// From OmO model catalogs. Never saved.
    pub auto_limits: HashMap<String, u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            order: StatKey::ALL.to_vec(),
            bar: HashSet::from(DEFAULT_BAR),
            row: HashSet::from(DEFAULT_ROW),
            cost: CostMode::Auto,
            ctx_limits: BTreeMap::new(),
            auto_limits: HashMap::new(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let mut cfg = Self::default();
        let mut bar_written = None;
        let mut row_written = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "bar" => bar_written = Some(stat_list(value)),
                "row" => row_written = Some(stat_list(value)),
                "cost" => {
                    if let Some(mode) = CostMode::parse(value) {
                        cfg.cost = mode;
                    }
                }
                _ => {
                    if let Some(model) = key.strip_prefix("ctx-limit.").filter(|model| !model.is_empty())
                        && let Ok(limit) = value.parse::<u64>()
                    {
                        cfg.ctx_limits.insert(model.to_string(), limit);
                    }
                }
            }
        }
        if let Some(list) = &bar_written {
            cfg.bar = list.iter().copied().collect();
        }
        if let Some(list) = &row_written {
            cfg.row = list.iter().copied().collect();
        }
        let bar_list = bar_written.unwrap_or_else(|| in_all_order(&cfg.bar));
        let row_list = row_written.unwrap_or_else(|| in_all_order(&cfg.row));
        cfg.order = compose_order(&bar_list, &row_list);
        cfg
    }

    pub fn to_text(&self) -> String {
        let mut text = String::from("# omo-scope stats config\n");
        text.push_str("bar = ");
        text.push_str(&format_stat_list(&self.bar_keys()));
        text.push('\n');
        text.push_str("row = ");
        text.push_str(&format_stat_list(&self.row_keys()));
        text.push('\n');
        text.push_str("cost = ");
        text.push_str(self.cost.name());
        text.push('\n');
        for (model, limit) in &self.ctx_limits {
            text.push_str(&format!("ctx-limit.{model} = {limit}\n"));
        }
        text
    }

    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let base = base.unwrap_or_else(|| crate::store::home().join(".config"));
        Some(base.join("omo-scope").join("config"))
    }

    pub fn load() -> Self {
        let mut cfg = match Self::path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        };
        let dir = crate::store::agent_dir();
        for name in ["models.json", "models-store.json"] {
            cfg.auto_limits.extend(read_catalog(&dir.join(name)));
        }
        cfg
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn limits_from_models(value: &Value) -> HashMap<String, u64> {
        let mut limits = HashMap::new();
        walk_models(value, &mut limits);
        limits
    }

    pub fn save(&self) -> anyhow::Result<PathBuf> {
        let path = Self::path().context("no config path")?;
        self.save_to(&path)?;
        Ok(path)
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let tmp = tmp_path(path);
        fs::write(&tmp, self.to_text()).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    pub fn bar_keys(&self) -> Vec<StatKey> {
        self.order
            .iter()
            .copied()
            .filter(|key| self.bar.contains(key))
            .collect()
    }

    pub fn row_keys(&self) -> Vec<StatKey> {
        self.order
            .iter()
            .copied()
            .filter(|key| self.row.contains(key))
            .collect()
    }

    pub fn set_bar_list(&mut self, list: &str) -> anyhow::Result<()> {
        let list = list.trim();
        if list.is_empty() || list == "none" {
            self.bar.clear();
            return Ok(());
        }
        let mut listed = Vec::new();
        for token in list.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let Some(key) = StatKey::parse(token) else {
                let valid = StatKey::ALL.map(StatKey::name).join(", ");
                bail!("unknown stat token '{token}' (valid: {valid})");
            };
            if !listed.contains(&key) {
                listed.push(key);
            }
        }
        if listed.is_empty() {
            self.bar.clear();
            return Ok(());
        }
        let mut order = Vec::with_capacity(StatKey::ALL.len());
        order.extend(listed.iter().copied());
        for key in &self.order {
            if !order.contains(key) {
                order.push(*key);
            }
        }
        for key in StatKey::ALL {
            if !order.contains(&key) {
                order.push(key);
            }
        }
        self.bar = listed.into_iter().collect();
        self.order = order;
        Ok(())
    }

    pub fn toggle_bar(&mut self, key: StatKey) {
        toggle(&mut self.bar, key);
    }

    pub fn toggle_row(&mut self, key: StatKey) {
        toggle(&mut self.row, key);
    }

    pub fn move_key(&mut self, i: usize, up: bool) -> usize {
        let n = self.order.len();
        if i >= n {
            return i;
        }
        let j = if up { i.wrapping_sub(1) } else { i + 1 };
        if j >= n {
            return i;
        }
        self.order.swap(i, j);
        j
    }

    pub fn ctx_limit(&self, model: &str) -> Option<u64> {
        self.ctx_limits
            .get(model)
            .or_else(|| self.auto_limits.get(model))
            .copied()
    }

    pub fn ctx_is_auto(&self, model: &str) -> bool {
        !self.ctx_limits.contains_key(model)
    }

    pub fn step_ctx_limit(&mut self, model: &str, up: bool) {
        let cur = self.ctx_limit(model);
        if up {
            let next = match cur {
                None => CTX_STEPS[0],
                Some(cur) => CTX_STEPS
                    .into_iter()
                    .find(|step| *step > cur)
                    .unwrap_or(CTX_STEPS[CTX_STEPS.len() - 1]),
            };
            self.ctx_limits.insert(model.to_string(), next);
            return;
        }
        let Some(cur) = cur else {
            self.ctx_limits.remove(model);
            return;
        };
        match CTX_STEPS.into_iter().rfind(|step| *step < cur) {
            Some(next) => {
                self.ctx_limits.insert(model.to_string(), next);
            }
            None => {
                self.ctx_limits.remove(model);
            }
        }
    }

    pub fn reset(&mut self) {
        let auto_limits = std::mem::take(&mut self.auto_limits);
        *self = Self::default();
        self.auto_limits = auto_limits;
    }
}

fn stat_list(value: &str) -> Vec<StatKey> {
    if value.is_empty() || value == "none" {
        return Vec::new();
    }
    let mut keys = Vec::new();
    for token in value.split(',') {
        let Some(key) = StatKey::parse(token) else {
            continue;
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn in_all_order(set: &HashSet<StatKey>) -> Vec<StatKey> {
    StatKey::ALL.into_iter().filter(|key| set.contains(key)).collect()
}

fn compose_order(bar: &[StatKey], row: &[StatKey]) -> Vec<StatKey> {
    let mut order = Vec::with_capacity(StatKey::ALL.len());
    for key in bar.iter().chain(row) {
        if !order.contains(key) {
            order.push(*key);
        }
    }
    for key in StatKey::ALL {
        if !order.contains(&key) {
            order.push(key);
        }
    }
    order
}

fn format_stat_list(keys: &[StatKey]) -> String {
    if keys.is_empty() {
        return "none".to_string();
    }
    keys.iter().map(|key| key.name()).collect::<Vec<_>>().join(", ")
}

fn toggle(set: &mut HashSet<StatKey>, key: StatKey) {
    if !set.remove(&key) {
        set.insert(key);
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

fn read_catalog(path: &Path) -> HashMap<String, u64> {
    let Ok(meta) = fs::metadata(path) else {
        return HashMap::new();
    };
    if !meta.is_file() || meta.len() > MAX_CATALOG_BYTES {
        return HashMap::new();
    }
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str::<Value>(&text)
        .map(|value| Config::limits_from_models(&value))
        .unwrap_or_default()
}

fn walk_models(value: &Value, out: &mut HashMap<String, u64>) {
    match value {
        Value::Array(items) => {
            for item in items {
                walk_models(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(Value::Array(models)) = map.get("models") {
                for model in models {
                    let Some(id) = model.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(window) = model.get("contextWindow").and_then(Value::as_u64) else {
                        continue;
                    };
                    out.insert(id.to_string(), window);
                }
            }
            for child in map.values() {
                walk_models(child, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
