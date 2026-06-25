//! Approximate per-model token pricing, for the cost estimate shown in the UI.
//!
//! These are list prices (USD per 1M tokens) and drift over time — they are a
//! convenience estimate, not a billing source of truth. The local provider is
//! always free. A user-editable override can be layered on later; for now the
//! table covers the configurable defaults and common families.

use crate::models::ProviderId;

/// (input, output) USD per 1M tokens, or `None` for free/unknown models.
pub fn price_per_million(provider: ProviderId, model: &str) -> Option<(f64, f64)> {
    if provider == ProviderId::Local {
        return None;
    }
    let m = model.to_lowercase();
    match provider {
        ProviderId::Anthropic => {
            if m.contains("opus") {
                Some((15.0, 75.0))
            } else if m.contains("haiku") {
                Some((1.0, 5.0))
            } else {
                // Sonnet and unknown Claude models default to Sonnet pricing.
                Some((3.0, 15.0))
            }
        }
        ProviderId::Gemini => {
            if m.contains("flash") {
                Some((0.30, 2.50))
            } else {
                // 2.5 Pro and unknown Gemini models default to Pro pricing.
                Some((1.25, 10.0))
            }
        }
        ProviderId::Local => None,
    }
}

/// Estimated USD cost of a call; 0.0 for free/unknown models.
pub fn cost_usd(provider: ProviderId, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
    match price_per_million(provider, model) {
        Some((pin, pout)) => {
            (input_tokens as f64 / 1_000_000.0) * pin
                + (output_tokens as f64 / 1_000_000.0) * pout
        }
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_is_free() {
        assert_eq!(cost_usd(ProviderId::Local, "llama3.1:8b", 1000, 1000), 0.0);
        assert!(price_per_million(ProviderId::Local, "anything").is_none());
    }

    #[test]
    fn opus_costs_more_than_flash() {
        let opus = cost_usd(ProviderId::Anthropic, "claude-opus-4-8", 1_000_000, 1_000_000);
        let flash = cost_usd(ProviderId::Gemini, "gemini-2.5-flash", 1_000_000, 1_000_000);
        assert!(opus > flash);
        // 15 + 75 per the table.
        assert!((opus - 90.0).abs() < 1e-9);
    }
}
