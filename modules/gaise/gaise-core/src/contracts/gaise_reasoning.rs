//! Provider-neutral reasoning ("thinking") effort vocabulary.
//!
//! `GaiseGenerationConfig::thinking_effort` stays a string on the wire so
//! callers can pass a vendor-native value, but adapters interpret it through
//! [`GaiseReasoningEffort`]: a canonical ladder, a fixed set of accepted
//! aliases, and a single rule for values a family cannot honour.
//!
//! | Canonical | Aliases (case-insensitive) | Meaning |
//! |---|---|---|
//! | `none` | `off`, `disabled`, `disable`, `false`, `0`, `no` | Turn reasoning off (where the family allows) |
//! | `auto` | `default`, `adaptive`, `on`, `true`, `enabled`, `yes`, `dynamic` | Reasoning on, provider chooses the depth |
//! | `minimal` | `min`, `lowest`, `very_low` | Smallest non-zero effort |
//! | `low` | — | |
//! | `medium` | `med`, `mid`, `moderate`, `standard`, `balanced` | |
//! | `high` | — | |
//! | `xhigh` | `x-high`, `x_high`, `extra_high`, `extra-high`, `very_high`, `very-high`, `xl` | |
//! | `max` | `maximum`, `xxhigh` | The vendor level literally named `max` where it exists |
//! | `ultra` | `ultracode`, `ultrathink`, `highest`, `extreme`, `unlimited` | **The highest level the model supports, whatever it is called.** Never sent verbatim |
//!
//! Anything else is [`GaiseReasoningEffort::Custom`] and is forwarded to the
//! provider verbatim, so a brand-new vendor level works before GAISe learns
//! its name. When a canonical level is not in a family's accepted set the
//! adapter snaps it to the **nearest** accepted level by rank, resolving ties
//! upward ([`GaiseReasoningEffort::clamp_to`]); `none` on a family that
//! cannot disable reasoning becomes that family's lowest level, and `ultra`
//! always becomes its highest (`max` when the family is unknown).

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GaiseReasoningEffort {
    None,
    Auto,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
    /// The highest level the target model supports. Resolved per family;
    /// never sent to a provider as-is.
    Ultra,
    /// A vendor-specific value GAISe does not recognise; forwarded as given.
    Custom(String),
}

impl GaiseReasoningEffort {
    /// The canonical ladder, lowest to highest, excluding `auto` and custom values.
    pub const LADDER: [GaiseReasoningEffort; 8] = [
        Self::None,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
        Self::Ultra,
    ];

    /// Parse a canonical name or alias. Never fails: unknown text becomes `Custom`.
    pub fn parse(value: &str) -> Self {
        let v = value.trim().to_ascii_lowercase();
        match v.as_str() {
            "none" | "off" | "disabled" | "disable" | "false" | "0" | "no" => Self::None,
            "auto" | "default" | "adaptive" | "on" | "true" | "enabled" | "yes" | "dynamic" => {
                Self::Auto
            }
            "minimal" | "min" | "lowest" | "very_low" | "very-low" => Self::Minimal,
            "low" => Self::Low,
            "medium" | "med" | "mid" | "moderate" | "standard" | "balanced" => Self::Medium,
            "high" => Self::High,
            "xhigh" | "x-high" | "x_high" | "extra_high" | "extra-high" | "very_high"
            | "very-high" | "xl" => Self::XHigh,
            "max" | "maximum" | "xxhigh" => Self::Max,
            "ultra" | "ultracode" | "ultrathink" | "highest" | "extreme" | "unlimited" => {
                Self::Ultra
            }
            _ => Self::Custom(value.trim().to_string()),
        }
    }

    /// Canonical lowercase name (`Custom` returns its raw text).
    pub fn as_str(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Auto => "auto",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
            Self::Custom(raw) => raw,
        }
    }

    /// Position on the ladder; `None` for `auto` and custom values.
    pub fn rank(&self) -> Option<usize> {
        Self::LADDER.iter().position(|l| l == self)
    }

    pub fn is_none(&self) -> bool {
        *self == Self::None
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }

    /// Snap this level onto `accepted` (a family's supported ladder levels):
    /// exact match wins; otherwise the nearest rank, ties resolving upward.
    /// `ultra` always resolves to the highest accepted level. `auto` and
    /// custom values are returned unchanged; an empty `accepted` list (an
    /// unknown family) returns the value unchanged except that `ultra`
    /// becomes `max`.
    pub fn clamp_to(&self, accepted: &[GaiseReasoningEffort]) -> GaiseReasoningEffort {
        if *self == Self::Ultra {
            return accepted
                .iter()
                .filter(|l| l.rank().is_some() && **l != Self::Ultra)
                .max_by_key(|l| l.rank())
                .cloned()
                .unwrap_or(Self::Max);
        }
        if accepted.is_empty() || accepted.contains(self) {
            return self.clone();
        }
        let Some(target) = self.rank() else {
            return self.clone();
        };
        accepted
            .iter()
            .filter_map(|level| level.rank().map(|rank| (rank, level)))
            .min_by_key(|(rank, _)| (rank.abs_diff(target), usize::from(*rank < target)))
            .map(|(_, level)| level.clone())
            .unwrap_or_else(|| self.clone())
    }

    /// Build an accepted-level list from canonical names (for family tables).
    pub fn levels(names: &[&str]) -> Vec<GaiseReasoningEffort> {
        names.iter().map(|n| Self::parse(n)).collect()
    }

    /// Approximate this level as a manual thinking-token budget for providers
    /// whose only control is a budget (Claude 4.5 and older, Gemini 2.5).
    /// `None` for `none` (disable), `auto` (provider default), and custom values.
    /// `max_budget` caps `max`/`ultra`.
    pub fn approximate_budget(&self, max_budget: usize) -> Option<usize> {
        let budget = match self {
            Self::Minimal => 1_024,
            Self::Low => 2_048,
            Self::Medium => 4_096,
            Self::High => 8_192,
            Self::XHigh => 16_384,
            Self::Max | Self::Ultra => max_budget,
            Self::None | Self::Auto | Self::Custom(_) => return None,
        };
        Some(budget.min(max_budget))
    }
}

impl fmt::Display for GaiseReasoningEffort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for GaiseReasoningEffort {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GaiseReasoningEffort {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(&raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_names_and_aliases() {
        for (input, expected) in [
            ("none", GaiseReasoningEffort::None),
            ("OFF", GaiseReasoningEffort::None),
            ("disabled", GaiseReasoningEffort::None),
            ("false", GaiseReasoningEffort::None),
            ("0", GaiseReasoningEffort::None),
            ("auto", GaiseReasoningEffort::Auto),
            ("adaptive", GaiseReasoningEffort::Auto),
            ("true", GaiseReasoningEffort::Auto),
            ("default", GaiseReasoningEffort::Auto),
            ("minimal", GaiseReasoningEffort::Minimal),
            ("min", GaiseReasoningEffort::Minimal),
            (" low ", GaiseReasoningEffort::Low),
            ("Medium", GaiseReasoningEffort::Medium),
            ("moderate", GaiseReasoningEffort::Medium),
            ("high", GaiseReasoningEffort::High),
            ("xhigh", GaiseReasoningEffort::XHigh),
            ("extra-high", GaiseReasoningEffort::XHigh),
            ("very_high", GaiseReasoningEffort::XHigh),
            ("max", GaiseReasoningEffort::Max),
            ("maximum", GaiseReasoningEffort::Max),
            ("ultra", GaiseReasoningEffort::Ultra),
            ("ultracode", GaiseReasoningEffort::Ultra),
            ("ultrathink", GaiseReasoningEffort::Ultra),
            ("highest", GaiseReasoningEffort::Ultra),
        ] {
            assert_eq!(GaiseReasoningEffort::parse(input), expected, "{input}");
        }
        assert_eq!(
            GaiseReasoningEffort::parse("deep_think_9000"),
            GaiseReasoningEffort::Custom("deep_think_9000".into())
        );
        assert_eq!(GaiseReasoningEffort::parse("xhigh").as_str(), "xhigh");
        assert_eq!(GaiseReasoningEffort::parse("ULTRA").to_string(), "ultra");
    }

    #[test]
    fn clamps_to_nearest_accepted_level() {
        let no_minimal = GaiseReasoningEffort::levels(&["low", "medium", "high"]);
        assert_eq!(GaiseReasoningEffort::None.clamp_to(&no_minimal), GaiseReasoningEffort::Low);
        assert_eq!(GaiseReasoningEffort::Minimal.clamp_to(&no_minimal), GaiseReasoningEffort::Low);
        assert_eq!(GaiseReasoningEffort::Max.clamp_to(&no_minimal), GaiseReasoningEffort::High);
        assert_eq!(GaiseReasoningEffort::Medium.clamp_to(&no_minimal), GaiseReasoningEffort::Medium);

        let image = GaiseReasoningEffort::levels(&["minimal", "high"]);
        assert_eq!(GaiseReasoningEffort::Low.clamp_to(&image), GaiseReasoningEffort::Minimal, "nearer");
        assert_eq!(GaiseReasoningEffort::Medium.clamp_to(&image), GaiseReasoningEffort::High, "nearer");

        let four_six = GaiseReasoningEffort::levels(&["low", "medium", "high", "max"]);
        assert_eq!(GaiseReasoningEffort::XHigh.clamp_to(&four_six), GaiseReasoningEffort::Max, "tie resolves upward");

        let gpt5 = GaiseReasoningEffort::levels(&["minimal", "low", "medium", "high"]);
        assert_eq!(GaiseReasoningEffort::None.clamp_to(&gpt5), GaiseReasoningEffort::Minimal);

        // Ultra resolves to the top of whatever the family offers.
        assert_eq!(GaiseReasoningEffort::Ultra.clamp_to(&no_minimal), GaiseReasoningEffort::High);
        assert_eq!(GaiseReasoningEffort::Ultra.clamp_to(&four_six), GaiseReasoningEffort::Max);
        assert_eq!(GaiseReasoningEffort::Ultra.clamp_to(&image), GaiseReasoningEffort::High);
        assert_eq!(GaiseReasoningEffort::Ultra.clamp_to(&[]), GaiseReasoningEffort::Max, "unknown family: max");
        let with_xhigh = GaiseReasoningEffort::levels(&["none", "low", "medium", "high", "xhigh"]);
        assert_eq!(GaiseReasoningEffort::Ultra.clamp_to(&with_xhigh), GaiseReasoningEffort::XHigh);

        // Auto and custom values never move; empty lists are pass-through.
        assert_eq!(GaiseReasoningEffort::Auto.clamp_to(&gpt5), GaiseReasoningEffort::Auto);
        let custom = GaiseReasoningEffort::Custom("ultra-deep".into());
        assert_eq!(custom.clamp_to(&gpt5), custom);
        assert_eq!(GaiseReasoningEffort::Max.clamp_to(&[]), GaiseReasoningEffort::Max);
    }

    #[test]
    fn approximates_budgets_for_budget_only_providers() {
        assert_eq!(GaiseReasoningEffort::Minimal.approximate_budget(32_000), Some(1_024));
        assert_eq!(GaiseReasoningEffort::High.approximate_budget(32_000), Some(8_192));
        assert_eq!(GaiseReasoningEffort::Ultra.approximate_budget(32_000), Some(32_000));
        assert_eq!(GaiseReasoningEffort::XHigh.approximate_budget(8_000), Some(8_000), "capped");
        assert_eq!(GaiseReasoningEffort::None.approximate_budget(32_000), None);
        assert_eq!(GaiseReasoningEffort::Auto.approximate_budget(32_000), None);
    }

    #[test]
    fn serde_round_trips_as_strings() {
        let json = serde_json::to_string(&GaiseReasoningEffort::XHigh).unwrap();
        assert_eq!(json, "\"xhigh\"");
        let parsed: GaiseReasoningEffort = serde_json::from_str("\"extra_high\"").unwrap();
        assert_eq!(parsed, GaiseReasoningEffort::XHigh);
        let custom: GaiseReasoningEffort = serde_json::from_str("\"vendor_level\"").unwrap();
        assert_eq!(serde_json::to_string(&custom).unwrap(), "\"vendor_level\"");
    }
}
