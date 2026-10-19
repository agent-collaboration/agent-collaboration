//! Display facts projected from typed quota observations. Pool membership,
//! observation freshness and row routing status are deliberately separate.
use codex_router_selection::burn_down::{AccountAvailability, QuotaEvidenceFreshness};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaAccountStatus {
    Preferred,
    Available,
    Held,
    Blocked,
    Stale,
    Unknown,
}
impl QuotaAccountStatus {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Preferred => "preferred",
            Self::Available => "available",
            Self::Held => "held",
            Self::Blocked => "blocked",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
    pub(crate) const fn glyph(self) -> &'static str {
        match self {
            Self::Preferred => "★",
            Self::Available => "✓",
            Self::Held => "◇",
            Self::Blocked => "⊘",
            Self::Stale => "◷",
            Self::Unknown => "?",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuotaRowStatus {
    Account(QuotaAccountStatus),
    CredentialUnavailable { notice: String },
}
impl QuotaRowStatus {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Account(status) => status.label(),
            Self::CredentialUnavailable { notice } => notice,
        }
    }
    pub(crate) fn compact(&self, width: usize) -> String {
        match self {
            Self::Account(status) => {
                let labeled = format!("{} {}", status.glyph(), status.label());
                if UnicodeWidthStr::width(labeled.as_str()) <= width {
                    labeled
                } else {
                    status.glyph().to_owned()
                }
            }
            Self::CredentialUnavailable { notice } => notice.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QuotaPoolCensus {
    pub usable: usize,
    pub reserve: usize,
    pub blocked: usize,
    pub unknown: usize,
    pub excluded: usize,
}
impl QuotaPoolCensus {
    pub(crate) fn observe(&mut self, availability: AccountAvailability) {
        let count = match availability {
            AccountAvailability::Usable => &mut self.usable,
            AccountAvailability::Reserve => &mut self.reserve,
            AccountAvailability::Blocked => &mut self.blocked,
            AccountAvailability::Unknown => &mut self.unknown,
            AccountAvailability::Excluded => &mut self.excluded,
        };
        *count = count.saturating_add(1);
    }
    pub(crate) fn total(self) -> usize {
        [
            self.usable,
            self.reserve,
            self.blocked,
            self.unknown,
            self.excluded,
        ]
        .into_iter()
        .fold(0, usize::saturating_add)
    }
    pub(crate) fn summary(self, width: usize) -> String {
        counter_group(
            &[
                ("Σ", "accounts", self.total().to_string()),
                ("✓", "usable", self.usable.to_string()),
                ("◇", "reserve", self.reserve.to_string()),
                ("⊘", "blocked", self.blocked.to_string()),
                ("?", "unknown", self.unknown.to_string()),
                ("×", "excluded", self.excluded.to_string()),
            ],
            width,
        )
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QuotaFreshnessCensus {
    pub fresh: usize,
    pub stale: usize,
    pub unknown: usize,
}
impl QuotaFreshnessCensus {
    pub(crate) fn observe(&mut self, freshness: QuotaEvidenceFreshness) {
        let count = match freshness {
            QuotaEvidenceFreshness::Fresh => &mut self.fresh,
            QuotaEvidenceFreshness::Stale => &mut self.stale,
            QuotaEvidenceFreshness::Unknown => &mut self.unknown,
        };
        *count = count.saturating_add(1);
    }
    pub(crate) fn summary(self, clients: Option<u32>, width: usize) -> String {
        counter_group(
            &[
                (
                    "⇄",
                    "clients",
                    clients.map_or_else(|| "?".to_owned(), |count| count.to_string()),
                ),
                ("●", "fresh", self.fresh.to_string()),
                ("◷", "stale", self.stale.to_string()),
                ("?", "unknown", self.unknown.to_string()),
            ],
            width,
        )
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct QuotaHeaderFacts {
    pub route_band: String,
    pub pool: QuotaPoolCensus,
    pub freshness: QuotaFreshnessCensus,
    pub observed_clients: Option<u32>,
    pub credential_notice: Option<String>,
}
fn counter_group(counters: &[(&str, &str, String)], width: usize) -> String {
    let labeled = counters
        .iter()
        .map(|(glyph, label, count)| format!("{glyph} {label} {count}"))
        .collect::<Vec<_>>()
        .join(" · ");
    if UnicodeWidthStr::width(labeled.as_str()) <= width {
        labeled
    } else {
        counters
            .iter()
            .map(|(glyph, _, count)| format!("{glyph} {count}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pool_and_freshness_counts_keep_independent_classifications() {
        let mut pool = QuotaPoolCensus::default();
        let mut freshness = QuotaFreshnessCensus::default();
        for (availability, observation) in [
            (AccountAvailability::Usable, QuotaEvidenceFreshness::Stale),
            (AccountAvailability::Reserve, QuotaEvidenceFreshness::Fresh),
            (
                AccountAvailability::Excluded,
                QuotaEvidenceFreshness::Unknown,
            ),
        ] {
            pool.observe(availability);
            freshness.observe(observation);
        }
        assert_eq!(
            pool,
            QuotaPoolCensus {
                usable: 1,
                reserve: 1,
                blocked: 0,
                unknown: 0,
                excluded: 1
            }
        );
        assert_eq!(
            freshness,
            QuotaFreshnessCensus {
                fresh: 1,
                stale: 1,
                unknown: 1
            }
        );
        assert_eq!(pool.total(), 3);
    }
    #[test]
    fn labels_collapse_before_any_count_or_separator_is_lost() {
        let census = QuotaPoolCensus {
            usable: 12345,
            reserve: 67,
            blocked: 8,
            unknown: 9,
            excluded: 0,
        };
        assert_eq!(
            census.summary(200),
            "Σ accounts 12429 · ✓ usable 12345 · ◇ reserve 67 · ⊘ blocked 8 · ? unknown 9 · × excluded 0"
        );
        assert_eq!(
            census.summary(1),
            "Σ 12429 · ✓ 12345 · ◇ 67 · ⊘ 8 · ? 9 · × 0"
        );
        assert_eq!(
            QuotaFreshnessCensus {
                fresh: 2,
                stale: 0,
                unknown: 1
            }
            .summary(None, 1),
            "⇄ ? · ● 2 · ◷ 0 · ? 1"
        );
    }
    #[test]
    fn row_status_keeps_preference_held_and_freshness_distinct_from_counts() {
        for (status, glyph, label) in [
            (QuotaAccountStatus::Preferred, "★", "preferred"),
            (QuotaAccountStatus::Available, "✓", "available"),
            (QuotaAccountStatus::Held, "◇", "held"),
            (QuotaAccountStatus::Blocked, "⊘", "blocked"),
            (QuotaAccountStatus::Stale, "◷", "stale"),
            (QuotaAccountStatus::Unknown, "?", "unknown"),
        ] {
            let row = QuotaRowStatus::Account(status);
            assert_eq!(row.label(), label);
            assert_eq!(row.compact(1), glyph);
            assert_eq!(row.compact(40), format!("{glyph} {label}"));
        }
        let unavailable = QuotaRowStatus::CredentialUnavailable {
            notice: "credentials locked".to_owned(),
        };
        assert_eq!(unavailable.compact(1), "credentials locked");
    }
}
