//! Router API receipts carry opaque continuations; native receipts keep raw keysets.
use crate::ProjectId;
use crate::{
    CanonicalHash, ChangeOutcome, DecisionEffect, DecisionEffectsHeader, EpochNumber, ReaderState,
    Refusal, RequestEffectsRefusal, RequestEffectsScope, RequestId, SchemaRefusal, ScopeRefusal,
    TextValidationError, TransferId,
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestEffectsRequest {
    pub scope: RequestEffectsScope,
    pub request: RequestId,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionEffectsPage {
    #[schemars(length(max = 256))]
    entries: Vec<DecisionEffect>,
    total_count: u64,
    digest: CanonicalHash,
    #[schemars(length(min = 1))]
    next: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EffectsPageWire {
    entries: Vec<DecisionEffect>,
    total_count: u64,
    digest: CanonicalHash,
    next: Option<String>,
}

impl DecisionEffectsPage {
    #[must_use]
    pub fn entries(&self) -> &[DecisionEffect] {
        &self.entries
    }
    #[must_use]
    pub fn total_count(&self) -> u64 {
        self.total_count
    }
    #[must_use]
    pub fn digest(&self) -> &CanonicalHash {
        &self.digest
    }
    #[must_use]
    pub fn next(&self) -> Option<&str> {
        self.next.as_deref()
    }

    pub fn from_page(
        entries: Vec<DecisionEffect>,
        header: &DecisionEffectsHeader,
        next: Option<String>,
    ) -> Result<Self, TextValidationError> {
        crate::decision_effects::validate_effect_page(&entries, header, next.is_some())?;
        if next.as_ref().is_some_and(String::is_empty) {
            return Err(TextValidationError {
                field: "next",
                requirement: "must be a nonempty opaque continuation",
            });
        }
        Ok(Self {
            entries,
            total_count: header.count,
            digest: header.digest.clone(),
            next,
        })
    }

    /// The boundary signs the native keyset; this conversion preserves the checked page.
    pub fn from_native(
        page: crate::DecisionEffectsPage,
        next: Option<String>,
    ) -> Result<Self, TextValidationError> {
        let (entries, header, raw_next) = page.into_parts();
        if raw_next.is_some() != next.is_some() {
            return Err(TextValidationError {
                field: "next",
                requirement: "must preserve native receipt continuation",
            });
        }
        Self::from_page(entries, &header, next)
    }
}

impl<'de> Deserialize<'de> for DecisionEffectsPage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = EffectsPageWire::deserialize(deserializer)?;
        Self::from_page(
            wire.entries,
            &DecisionEffectsHeader {
                count: wire.total_count,
                digest: wire.digest,
            },
            wire.next,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RequestDecision {
    Accepted {
        effects: DecisionEffectsPage,
        change: ChangeOutcome,
    },
    Rejected {
        reasons: Vec<Refusal>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "decision",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RequestResult {
    Decided(RequestDecision),
    ConflictingRequest,
    NotWriter {
        current: Option<EpochNumber>,
    },
    /// Central has no holder; no post was admitted or queued by the sender.
    NotHeld,
    /// Presence is central's observation, converted to the board's UTC time format.
    HolderUnreachable {
        last_seen: Option<DateTime<Utc>>,
    },
    /// The holder may have committed; retry only with the original request identity.
    Unanswered,
    LeaseExpired {
        expired_at: DateTime<Utc>,
    },
    TransferInProgress {
        transfer: TransferId,
    },
    SchemaNotReady(SchemaRefusal),
    PendingUnreachable,
    ProjectCreationPending {
        project: ProjectId,
    },
    Overloaded,
    ScopeUnavailable(ScopeRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteReply {
    pub result: RequestResult,
    pub reader: Option<ReaderState>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RequestEffectsRead {
    Decision { decision: RequestDecision },
    Refused { reason: RequestEffectsRefusal },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Test assertions must fail the test while fixture errors propagate."
    )] // Assertions check wire values; Result propagates codec failures.
    fn forwarding_outcomes_are_public_receipts_without_a_ledger_decision()
    -> Result<(), serde_json::Error> {
        for value in [
            json!({"kind":"notHeld"}),
            json!({"kind":"holderUnreachable","decision":{"lastSeen":"2026-10-22T00:00:00Z"}}),
            json!({"kind":"holderUnreachable","decision":{"lastSeen":null}}),
            json!({"kind":"unanswered"}),
        ] {
            let reply: RequestResult = serde_json::from_value(value.clone())?;
            assert_eq!(serde_json::to_value(reply)?, value);
        }
        assert!(
            serde_json::from_value::<RequestResult>(
                json!({"kind":"holderUnreachable","decision":{"lastSeen":42}})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<RequestResult>(
                json!({"kind":"unanswered","decision":{"accepted":true}})
            )
            .is_err()
        );
        Ok(())
    }
}
