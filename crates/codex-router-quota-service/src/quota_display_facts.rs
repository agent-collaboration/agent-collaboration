//! Display facts projected from typed quota observations. Pool membership,
//! observation freshness and row routing status are deliberately separate.
use codex_router_selection::burn_down::{AccountAvailability, QuotaEvidenceFreshness};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaAccountStatus {
    Preferred,
    Available,
    Held,
    Blocked,
    Stale,
}
impl QuotaAccountStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Preferred => "preferred",
            Self::Available => "available",
            Self::Held => "held",
            Self::Blocked => "blocked",
            Self::Stale => "stale",
        }
    }
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Preferred => "★",
            Self::Available => "✓",
            Self::Held => "◇",
            Self::Blocked => "⊘",
            Self::Stale => "◷",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaRowStatus {
    Account(QuotaAccountStatus),
    CredentialUnavailable { notice: String },
}
impl QuotaRowStatus {
    pub fn label(&self) -> &str {
        match self {
            Self::Account(status) => status.label(),
            Self::CredentialUnavailable { notice } => notice,
        }
    }
    pub fn compact(&self, width: usize) -> String {
        match self {
            Self::Account(status) => {
                let labeled = format!("{} {}", status.glyph(), status.label());
                if UnicodeWidthStr::width(labeled.as_str()) <= width {
                    labeled
                } else if UnicodeWidthStr::width(status.label()) <= width {
                    status.label().to_owned()
                } else {
                    status.glyph().to_owned()
                }
            }
            Self::CredentialUnavailable { notice } => notice.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QuotaPoolCensus {
    pub usable: usize,
    pub reserve: usize,
    pub blocked: usize,
    pub unknown: usize,
    pub excluded: usize,
}
impl QuotaPoolCensus {
    pub fn observe(&mut self, availability: AccountAvailability) {
        let count = match availability {
            AccountAvailability::Usable => &mut self.usable,
            AccountAvailability::Reserve => &mut self.reserve,
            AccountAvailability::Blocked => &mut self.blocked,
            AccountAvailability::Unknown => &mut self.unknown,
            AccountAvailability::Excluded => &mut self.excluded,
        };
        *count = count.saturating_add(1);
    }
    pub fn total(self) -> usize {
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
    pub fn summary(self, width: usize) -> String {
        let counters = [
            ("Accounts", self.total()),
            ("Usable", self.usable),
            ("Reserve", self.reserve),
            ("Blocked", self.blocked),
            ("Pool unknown", self.unknown),
            ("Excluded", self.excluded),
        ];
        let full = counters
            .iter()
            .map(|(label, count)| format!("{label} {count}"))
            .collect::<Vec<_>>()
            .join(" · ");
        if UnicodeWidthStr::width(full.as_str()) <= width {
            return full;
        }
        counters
            .iter()
            .enumerate()
            .filter(|(index, (_, count))| *index < 2 || *count > 0)
            .map(|(_, (label, count))| format!("{label} {count}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QuotaFreshnessCensus {
    pub fresh: usize,
    pub stale: usize,
    pub unknown: usize,
}
impl QuotaFreshnessCensus {
    pub fn observe(&mut self, freshness: QuotaEvidenceFreshness) {
        let count = match freshness {
            QuotaEvidenceFreshness::Fresh => &mut self.fresh,
            QuotaEvidenceFreshness::Stale => &mut self.stale,
            QuotaEvidenceFreshness::Unknown => &mut self.unknown,
        };
        *count = count.saturating_add(1);
    }
    pub fn summary(self, clients: Option<u32>) -> String {
        let clients = clients.map_or_else(|| "unknown".to_owned(), |count| count.to_string());
        format!(
            "Clients {clients} · Fresh {} · Stale {} · Data unknown {}",
            self.fresh, self.stale, self.unknown
        )
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuotaHeaderFacts {
    pub route_band: String,
    pub pool: QuotaPoolCensus,
    pub freshness: QuotaFreshnessCensus,
    pub observed_clients: Option<u32>,
    pub credential_notice: Option<String>,
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
    fn compact_counts_keep_names_and_nonzero_problems_with_complete_full_summary() {
        let census = QuotaPoolCensus {
            usable: 12345,
            reserve: 67,
            blocked: 8,
            unknown: 9,
            excluded: 0,
        };
        assert_eq!(
            census.summary(200),
            "Accounts 12429 · Usable 12345 · Reserve 67 · Blocked 8 · Pool unknown 9 · Excluded 0"
        );
        assert_eq!(
            census.summary(1),
            "Accounts 12429 · Usable 12345 · Reserve 67 · Blocked 8 · Pool unknown 9"
        );
        assert_eq!(
            QuotaFreshnessCensus {
                fresh: 2,
                stale: 0,
                unknown: 1
            }
            .summary(None),
            "Clients unknown · Fresh 2 · Stale 0 · Data unknown 1"
        );
    }
    #[test]
    fn compact_status_uses_a_complete_fitting_label_before_falling_back_to_glyph() {
        for (status, at_seven, at_nine) in [
            (QuotaAccountStatus::Preferred, "★", "preferred"),
            (QuotaAccountStatus::Available, "✓", "available"),
            (QuotaAccountStatus::Held, "◇ held", "◇ held"),
            (QuotaAccountStatus::Blocked, "blocked", "⊘ blocked"),
            (QuotaAccountStatus::Stale, "◷ stale", "◷ stale"),
        ] {
            let row = QuotaRowStatus::Account(status);
            assert_eq!(row.compact(7), at_seven, "{status:?} at seven cells");
            assert_eq!(row.compact(9), at_nine, "{status:?} at nine cells");
        }
    }

    #[test]
    fn row_status_keeps_preference_held_and_freshness_distinct_from_counts() {
        for (status, glyph, label) in [
            (QuotaAccountStatus::Preferred, "★", "preferred"),
            (QuotaAccountStatus::Available, "✓", "available"),
            (QuotaAccountStatus::Held, "◇", "held"),
            (QuotaAccountStatus::Blocked, "⊘", "blocked"),
            (QuotaAccountStatus::Stale, "◷", "stale"),
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
