use jiff::Timestamp;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatKey {
    Turns,
    Tools,
    Tok,
    Io,
    Reasoning,
    Tps,
    Cache,
    Ctx,
    Compact,
    Cost,
}

impl StatKey {
    pub const ALL: [StatKey; 10] = [
        StatKey::Turns,
        StatKey::Tools,
        StatKey::Tps,
        StatKey::Ctx,
        StatKey::Cache,
        StatKey::Cost,
        StatKey::Io,
        StatKey::Reasoning,
        StatKey::Compact,
        StatKey::Tok,
    ];

    pub fn name(self) -> &'static str {
        match self {
            StatKey::Turns => "turns",
            StatKey::Tools => "tools",
            StatKey::Tok => "tok",
            StatKey::Io => "io",
            StatKey::Reasoning => "reasoning",
            StatKey::Tps => "tps",
            StatKey::Cache => "cache",
            StatKey::Ctx => "ctx",
            StatKey::Compact => "compact",
            StatKey::Cost => "cost",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            StatKey::Turns => "turns",
            StatKey::Tools => "tools",
            StatKey::Tok => "total tok",
            StatKey::Io => "in/out",
            StatKey::Reasoning => "reasoning",
            StatKey::Tps => "tok/s",
            StatKey::Cache => "cache",
            StatKey::Ctx => "context",
            StatKey::Compact => "compactions",
            StatKey::Cost => "cost",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == s.trim())
    }
}

/// Cost display for subscription providers only; other providers always show plain cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CostMode {
    /// `~$1.23`: list-price estimate, not real spend.
    #[default]
    Auto,
    Always,
    Never,
}

impl CostMode {
    pub fn name(self) -> &'static str {
        match self {
            CostMode::Auto => "auto",
            CostMode::Always => "always",
            CostMode::Never => "never",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [CostMode::Auto, CostMode::Always, CostMode::Never]
            .into_iter()
            .find(|m| m.name() == s.trim())
    }

    pub fn next(self) -> Self {
        match self {
            CostMode::Auto => CostMode::Always,
            CostMode::Always => CostMode::Never,
            CostMode::Never => CostMode::Auto,
        }
    }
}

/// Usage numbers for one task. Every field is optional: `None` means unknown, never zero.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub turns: Option<u64>,
    pub tool_calls: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub reasoning: Option<u64>,
    pub total: Option<u64>,
    pub tps: Option<f64>,
    pub cost: Option<f64>,
    /// 0.0..=1.0
    pub cache_rate: Option<f64>,
    /// input + cacheRead + cacheWrite of the most recent request.
    pub context: Option<u64>,
    pub compactions: Option<u64>,
    pub subscription: bool,
    pub partial: bool,
}

impl Stats {
    pub fn from_run_stats(v: &Value, provider: &str) -> Self {
        Self {
            turns: v["turns"].as_u64(),
            tool_calls: v["tool_calls"].as_u64(),
            input: v["input_tokens"].as_u64(),
            output: v["output_tokens"].as_u64(),
            cache_read: v["cache_read_tokens"].as_u64(),
            cache_write: v["cache_write_tokens"].as_u64(),
            reasoning: None,
            total: v["total_tokens"].as_u64(),
            tps: v["tokens_per_second"].as_f64(),
            cost: v["cost_usd"].as_f64(),
            cache_rate: v["cache_hit_rate_run"].as_f64(),
            context: None,
            compactions: None,
            subscription: provider.contains("subscription"),
            partial: status_open(v, "token_status", "complete") || status_open(v, "cost_status", "reported"),
        }
    }

    /// Recorded values win; live fills only what the task file has not reported yet.
    pub fn merge(&self, live: &Stats) -> Stats {
        Stats {
            turns: self.turns.or(live.turns),
            tool_calls: self.tool_calls.or(live.tool_calls),
            input: self.input.or(live.input),
            output: self.output.or(live.output),
            cache_read: self.cache_read.or(live.cache_read),
            cache_write: self.cache_write.or(live.cache_write),
            reasoning: self.reasoning.or(live.reasoning),
            total: self.total.or(live.total),
            tps: self.tps.or(live.tps),
            cost: self.cost.or(live.cost),
            cache_rate: self.cache_rate.or(live.cache_rate),
            context: self.context.or(live.context),
            compactions: self.compactions.or(live.compactions),
            subscription: self.subscription || live.subscription,
            partial: self.partial,
        }
    }

    pub fn render(&self, key: StatKey, cost: CostMode, ctx_limit: Option<u64>) -> Option<String> {
        let text = match key {
            StatKey::Turns => {
                let n = self.turns?;
                format!("{n} turns")
            }
            StatKey::Tools => {
                let n = self.tool_calls?;
                format!("{n} tools")
            }
            StatKey::Tok => {
                let shown = Self::fmt_count(self.total?);
                let mut text = format!("{shown} tok");
                if self.partial {
                    text.push('?');
                }
                text
            }
            StatKey::Io => {
                let inn = Self::fmt_count(self.input?);
                let out = Self::fmt_count(self.output?);
                format!("{inn} in \u{00b7} {out} out")
            }
            StatKey::Reasoning => {
                let n = self.reasoning?;
                if n == 0 {
                    return None;
                }
                let shown = Self::fmt_count(n);
                format!("{shown} think")
            }
            StatKey::Tps => {
                let n = self.tps?.round();
                format!("{n} tok/s")
            }
            StatKey::Cache => {
                let pct = (self.cache_rate? * 100.0).round();
                format!("{pct}% cache")
            }
            StatKey::Ctx => {
                let ctx = self.context?;
                let shown = Self::fmt_count(ctx);
                match ctx_limit {
                    Some(limit) if limit > 0 => {
                        let cap = Self::fmt_count(limit);
                        let pct = (u128::from(ctx) * 100 + u128::from(limit) / 2) / u128::from(limit);
                        format!("{shown}/{cap} ctx {pct}%")
                    }
                    _ => format!("{shown} ctx"),
                }
            }
            StatKey::Compact => {
                let n = self.compactions?;
                if n == 0 {
                    return None;
                }
                format!("\u{21e3}{n}")
            }
            StatKey::Cost => return self.render_cost(cost),
        };
        Some(text)
    }

    pub fn fmt_count(n: u64) -> String {
        match n {
            0..1_000 => n.to_string(),
            1_000..10_000 => {
                let tenths = n / 100 + u64::from(n % 100 >= 50);
                let whole = tenths / 10;
                let frac = tenths % 10;
                format!("{whole}.{frac}k")
            }
            10_000..1_000_000 => {
                let thousands = n / 1_000 + u64::from(n % 1_000 >= 500);
                format!("{thousands}k")
            }
            1_000_000..10_000_000 => {
                let tenths = n / 100_000 + u64::from(n % 100_000 >= 50_000);
                let whole = tenths / 10;
                let frac = tenths % 10;
                format!("{whole}.{frac}M")
            }
            _ => {
                let millions = n / 1_000_000 + u64::from(n % 1_000_000 >= 500_000);
                format!("{millions}M")
            }
        }
    }

    fn render_cost(&self, mode: CostMode) -> Option<String> {
        if self.subscription && mode == CostMode::Never {
            return None;
        }
        let amount = self.cost?;
        let mut text = if self.subscription && mode == CostMode::Auto {
            format!("~${amount:.2}")
        } else {
            format!("${amount:.2}")
        };
        if self.partial {
            text.push('?');
        }
        Some(text)
    }
}

fn status_open(v: &Value, field: &str, done: &str) -> bool {
    match v.get(field) {
        Some(Value::String(s)) => s != done,
        Some(other) if !other.is_null() => true,
        _ => false,
    }
}

fn u64_of(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

fn epoch_ms(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
}

fn iso_ms(v: &Value) -> Option<i64> {
    v.as_str()
        .and_then(|s| s.parse::<Timestamp>().ok())
        .map(|t| t.as_millisecond())
}

#[derive(Debug, Default, Clone)]
pub struct Usage {
    turns: u64,
    tool_calls: u64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    reasoning: u64,
    total: u64,
    cost: f64,
    gen_ms: u64,
    context: u64,
    compactions: u64,
    subscription: bool,
}

impl Usage {
    pub fn push(&mut self, v: &Value) {
        match v["type"].as_str() {
            Some("compaction") => self.compactions = self.compactions.saturating_add(1),
            Some("message") => self.push_assistant(&v["message"], v),
            _ => {}
        }
    }

    pub fn stats(&self) -> Stats {
        let compactions = (self.compactions > 0).then_some(self.compactions);
        if self.turns == 0 {
            return Stats {
                compactions,
                ..Stats::default()
            };
        }
        let denom = self.input + self.cache_read + self.cache_write;
        Stats {
            turns: Some(self.turns),
            tool_calls: Some(self.tool_calls),
            input: Some(self.input),
            output: Some(self.output),
            cache_read: Some(self.cache_read),
            cache_write: Some(self.cache_write),
            reasoning: Some(self.reasoning),
            total: Some(self.total),
            tps: (self.gen_ms > 0).then(|| self.output as f64 / (self.gen_ms as f64 / 1000.0)),
            cost: Some(self.cost),
            cache_rate: (denom > 0).then(|| self.cache_read as f64 / denom as f64),
            context: Some(self.context),
            compactions,
            subscription: self.subscription,
            partial: false,
        }
    }

    fn push_assistant(&mut self, message: &Value, line: &Value) {
        if message["role"].as_str() != Some("assistant") || !message["usage"].is_object() {
            return;
        }
        let usage = &message["usage"];
        self.turns = self.turns.saturating_add(1);
        self.tool_calls = self.tool_calls.saturating_add(tool_calls(&message["content"]));
        self.input = self.input.saturating_add(u64_of(&usage["input"]));
        self.output = self.output.saturating_add(u64_of(&usage["output"]));
        self.cache_read = self.cache_read.saturating_add(u64_of(&usage["cacheRead"]));
        self.cache_write = self.cache_write.saturating_add(u64_of(&usage["cacheWrite"]));
        self.reasoning = self.reasoning.saturating_add(u64_of(&usage["reasoning"]));
        self.total = self.total.saturating_add(u64_of(&usage["totalTokens"]));
        self.cost += usage["cost"]["total"].as_f64().unwrap_or(0.0);
        if let (Some(start), Some(end)) = (epoch_ms(&message["timestamp"]), iso_ms(&line["timestamp"]))
            && let Ok(delta) = u64::try_from(end - start)
            && delta > 0
        {
            self.gen_ms = self.gen_ms.saturating_add(delta);
        }
        self.context = u64_of(&usage["input"])
            .saturating_add(u64_of(&usage["cacheRead"]))
            .saturating_add(u64_of(&usage["cacheWrite"]));
        self.subscription |= message["provider"].as_str().is_some_and(|p| p.contains("subscription"));
    }
}

fn tool_calls(content: &Value) -> u64 {
    content
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == "toolCall")
                .fold(0_u64, |n, _| n.saturating_add(1))
        })
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
