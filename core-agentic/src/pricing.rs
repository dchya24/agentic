//! Token pricing for cost estimation.
//!
//! Pricing data is **not authoritative** — it's a best-effort mapping of
//! commonly used models to their public per-token rates so the agent loop
//! can show users a running cost estimate and enforce an optional budget
//! cap. Rates are stored in USD per 1M tokens (matching how providers
//! typically publish them) and converted to per-token at lookup time.
//!
//! Lookup strategy:
//!   1. Exact model-name match (case-insensitive).
//!   2. Prefix/contains fallback against the canonical names below
//!      (e.g. `gpt-4o-mini-2024-07-18` → `gpt-4o-mini`).
//!   3. Unknown → `None`. Callers should treat this as "cost unavailable"
//!      and skip budget enforcement for that turn (with a warning).
//!
//! Rates last reviewed: 2026-05. To update, edit `pricing_table()`.
//! Users who want their own rates can override per-model via
//! `Config.pricing` (see `core_agentic::Config`).

use serde::{Deserialize, Serialize};

/// Per-token rates for a model.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ModelPricing {
    /// USD per 1M input (prompt) tokens.
    pub input_per_million: f64,
    /// USD per 1M output (completion) tokens.
    pub output_per_million: f64,
}

impl ModelPricing {
    pub const fn new(input_per_million: f64, output_per_million: f64) -> Self {
        Self {
            input_per_million,
            output_per_million,
        }
    }

    /// Cost in USD for a given (input_tokens, output_tokens) pair.
    pub fn cost_usd(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_million;
        input_cost + output_cost
    }
}

/// Built-in pricing table. Keys are canonical model names matched against
/// the runtime model id with `.eq_ignore_ascii_case` first, then by
/// `contains` for vendor-suffixed variants like
/// `claude-3-5-sonnet-20241022`.
fn pricing_table() -> &'static [(&'static str, ModelPricing)] {
    static TABLE: &[(&str, ModelPricing)] = &[
        // ── OpenAI ───────────────────────────────────────────
        ("gpt-4o", ModelPricing::new(2.50, 10.00)),
        ("gpt-4o-mini", ModelPricing::new(0.15, 0.60)),
        ("gpt-4-turbo", ModelPricing::new(10.00, 30.00)),
        ("gpt-4", ModelPricing::new(30.00, 60.00)),
        ("gpt-3.5-turbo", ModelPricing::new(0.50, 1.50)),
        ("o1", ModelPricing::new(15.00, 60.00)),
        ("o1-mini", ModelPricing::new(3.00, 12.00)),
        // ── Anthropic ────────────────────────────────────────
        ("claude-3-5-sonnet", ModelPricing::new(3.00, 15.00)),
        ("claude-3-5-haiku", ModelPricing::new(0.80, 4.00)),
        ("claude-3-opus", ModelPricing::new(15.00, 75.00)),
        ("claude-3-sonnet", ModelPricing::new(3.00, 15.00)),
        ("claude-3-haiku", ModelPricing::new(0.25, 1.25)),
        // ── DeepSeek / GLM (community rates) ─────────────────
        ("deepseek-chat", ModelPricing::new(0.27, 1.10)),
        ("deepseek-coder", ModelPricing::new(0.14, 0.28)),
        ("glm-4", ModelPricing::new(0.50, 1.50)),
        ("glm-4.7", ModelPricing::new(0.50, 1.50)),
    ];
    TABLE
}

/// Look up pricing for a model name. Falls back to a `contains` match
/// against the canonical entries above so dated/vendor-suffixed model
/// names work without an exact entry.
pub fn lookup(model: &str) -> Option<ModelPricing> {
    let model_lower = model.to_lowercase();
    let table = pricing_table();

    // Exact match first.
    for (name, price) in table {
        if name.eq_ignore_ascii_case(model) {
            return Some(*price);
        }
    }

    // Then prefix/contains. Pick the longest match so
    // "claude-3-5-sonnet-20241022" prefers "claude-3-5-sonnet" over
    // "claude-3-sonnet".
    let mut best: Option<(&str, ModelPricing)> = None;
    for (name, price) in table {
        if model_lower.contains(&name.to_lowercase()) {
            match best {
                Some((current, _)) if current.len() >= name.len() => {}
                _ => best = Some((name, *price)),
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_zero_for_zero_tokens() {
        let p = ModelPricing::new(2.50, 10.00);
        assert_eq!(p.cost_usd(0, 0), 0.0);
    }

    #[test]
    fn cost_basic_arithmetic() {
        let p = ModelPricing::new(2.50, 10.00);
        // 1M input + 1M output = $2.50 + $10.00 = $12.50
        let cost = p.cost_usd(1_000_000, 1_000_000);
        assert!((cost - 12.50).abs() < 1e-9);
    }

    #[test]
    fn cost_fractional_tokens() {
        let p = ModelPricing::new(2.50, 10.00);
        // 100 input tokens at $2.50/M = $0.00025
        let cost = p.cost_usd(100, 0);
        assert!((cost - 0.00025).abs() < 1e-9);
    }

    #[test]
    fn lookup_exact_match_case_insensitive() {
        let p = lookup("gpt-4o").expect("known");
        assert_eq!(p.input_per_million, 2.50);
        let p = lookup("GPT-4O").expect("known");
        assert_eq!(p.input_per_million, 2.50);
    }

    #[test]
    fn lookup_prefers_longest_contains_match() {
        // "claude-3-5-sonnet-20241022" should map to claude-3-5-sonnet
        // (3.00 / 15.00), not the shorter claude-3-sonnet (also 3.00 /
        // 15.00 but for a different model). They happen to share rates,
        // so assert the principle on a structurally distinct case.
        let p = lookup("gpt-4o-mini-2024-07-18").expect("known");
        // gpt-4o-mini is 0.15 / 0.60; gpt-4o is 2.50 / 10.00. Must pick
        // the longer name.
        assert_eq!(p.input_per_million, 0.15);
        assert_eq!(p.output_per_million, 0.60);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("nonexistent-model-9000").is_none());
    }

    #[test]
    fn lookup_handles_dated_anthropic_suffix() {
        let p = lookup("claude-3-5-sonnet-20241022").expect("known");
        assert_eq!(p.input_per_million, 3.00);
        assert_eq!(p.output_per_million, 15.00);
    }

    #[test]
    fn lookup_handles_dated_haiku() {
        let p = lookup("claude-3-5-haiku-20241022").expect("known");
        assert_eq!(p.input_per_million, 0.80);
    }
}
