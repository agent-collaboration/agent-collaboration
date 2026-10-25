//! Immutable ledger commitments are read through bounded, request-scoped pages.
use crate::{
    CanonicalHash, ChangeOutcome, FeedPosition, ProjectId, RecordPlacement, RecordRef,
    RequestDecision, RequestId, StoredDecision, TextValidationError, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

pub type DecisionEffect = (RecordRef, RecordPlacement);
pub const MAX_EFFECT_PAGE_ENTRIES: usize = 256;

/// A Fleet receipt names a Fleet record at its original Fleet placement.
pub fn validate_fleet_decision_effect(effect: &DecisionEffect) -> Result<(), TextValidationError> {
    if !matches!(effect.1, RecordPlacement::Fleet(_))
        || !matches!(
            effect.0.kind,
            crate::RecordKind::Workspace
                | crate::RecordKind::Board
                | crate::RecordKind::Adr
                | crate::RecordKind::Project
        )
    {
        return Err(TextValidationError {
            field: "effect",
            requirement: "must name a Fleet record with a Fleet placement",
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionEffectsHeader {
    pub count: u64,
    pub digest: CanonicalHash,
}

impl DecisionEffectsHeader {
    #[must_use]
    pub fn from_full(effects: &[DecisionEffect]) -> Self {
        const {
            assert!(usize::BITS <= u64::BITS);
        }
        // The width assertion proves this conversion lossless on every target.
        Self {
            count: effects.len() as u64,
            digest: crate::abridged_evidence::evidence_digest(effects),
        }
    }
    #[must_use]
    pub fn matches_full(&self, effects: &[DecisionEffect]) -> bool {
        *self == Self::from_full(effects)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RequestEffectsScope {
    Project { project_id: ProjectId },
    Fleet { workspace_id: WorkspaceId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RequestEffectsCursor {
    Project { position: FeedPosition },
    Fleet { ordinal: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestEffectsRequest {
    pub scope: RequestEffectsScope,
    pub request: RequestId,
    pub after: Option<RequestEffectsCursor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RequestEffectsRefusal {
    UnknownRequest,
    RequestNotInScope,
    CursorInvalid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RequestEffectsRead {
    Decision { decision: RequestDecision },
    Refused { reason: RequestEffectsRefusal },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionEffectsPage {
    #[schemars(length(max = 256))]
    entries: Vec<DecisionEffect>,
    total_count: u64,
    digest: CanonicalHash,
    next: Option<RequestEffectsCursor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EffectsPageWire {
    entries: Vec<DecisionEffect>,
    total_count: u64,
    digest: CanonicalHash,
    next: Option<RequestEffectsCursor>,
}

impl DecisionEffectsPage {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<DecisionEffect>,
        DecisionEffectsHeader,
        Option<RequestEffectsCursor>,
    ) {
        (
            self.entries,
            DecisionEffectsHeader {
                count: self.total_count,
                digest: self.digest,
            },
            self.next,
        )
    }

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
    pub fn next(&self) -> Option<&RequestEffectsCursor> {
        self.next.as_ref()
    }

    pub fn from_full(
        effects: Vec<DecisionEffect>,
        header: &DecisionEffectsHeader,
        after: Option<&RequestEffectsCursor>,
    ) -> Result<Self, TextValidationError> {
        if !header.matches_full(&effects) {
            return Err(invalid("effects"));
        }
        let project = match effects.first().map(|(_, position)| position) {
            Some(RecordPlacement::Project(_)) => true,
            Some(RecordPlacement::Fleet(_)) => false,
            Some(RecordPlacement::Unsequenced) => return Err(invalid("effects")),
            None => {
                return if after.is_none() {
                    Ok(Self {
                        entries: Vec::new(),
                        total_count: header.count,
                        digest: header.digest.clone(),
                        next: None,
                    })
                } else {
                    Err(invalid("after"))
                };
            }
        };
        if effects.iter().any(|(_, position)| {
            !matches!(
                (project, position),
                (true, RecordPlacement::Project(_)) | (false, RecordPlacement::Fleet(_))
            )
        }) {
            return Err(invalid("effects"));
        }
        let start = match after {
            None => 0,
            Some(RequestEffectsCursor::Project { position }) if project => effects
                .iter()
                .position(|(_, placement)| {
                    placement
                        .project_cursor()
                        .is_some_and(|cursor| cursor.position == *position)
                })
                .map(|index| index + 1)
                .ok_or_else(|| invalid("after"))?,
            Some(RequestEffectsCursor::Fleet { ordinal }) if !project => usize::try_from(*ordinal)
                .ok()
                .filter(|index| *index < effects.len())
                .map(|index| index + 1)
                .ok_or_else(|| invalid("after"))?,
            Some(_) => return Err(invalid("after")),
        };
        let end = start
            .saturating_add(MAX_EFFECT_PAGE_ENTRIES)
            .min(effects.len());
        let next = if end < effects.len() {
            let last_ordinal = end.checked_sub(1).ok_or_else(|| invalid("effects"))?;
            Some(if project {
                RequestEffectsCursor::Project {
                    position: effects
                        .get(last_ordinal)
                        .ok_or_else(|| invalid("effects"))?
                        .1
                        .project_cursor()
                        .ok_or_else(|| invalid("effects"))?
                        .position,
                }
            } else {
                RequestEffectsCursor::Fleet {
                    ordinal: u64::try_from(last_ordinal).map_err(|_| invalid("effects"))?,
                }
            })
        } else {
            None
        };
        Self::from_page(
            effects.into_iter().skip(start).take(end - start).collect(),
            header,
            next,
        )
    }
}

fn invalid(field: &'static str) -> TextValidationError {
    TextValidationError {
        field,
        requirement: "must describe the immutable ordered decision receipt",
    }
}

impl DecisionEffectsPage {
    pub fn from_page(
        entries: Vec<DecisionEffect>,
        header: &DecisionEffectsHeader,
        next: Option<RequestEffectsCursor>,
    ) -> Result<Self, TextValidationError> {
        let wire = EffectsPageWire {
            entries,
            total_count: header.count,
            digest: header.digest.clone(),
            next,
        };
        let project = validate_effect_page(&wire.entries, header, wire.next.is_some())?;
        let next_valid = match &wire.next {
            None => true,
            Some(RequestEffectsCursor::Project { position }) => {
                project == Some(true)
                    && wire
                        .entries
                        .last()
                        .and_then(|(_, placement)| placement.project_cursor())
                        .is_some_and(|cursor| cursor.position == *position)
            }
            Some(RequestEffectsCursor::Fleet { ordinal }) => {
                project == Some(false) && *ordinal < wire.total_count.saturating_sub(1)
            }
        };
        if !next_valid {
            return Err(invalid("next"));
        }
        Ok(Self {
            entries: wire.entries,
            total_count: wire.total_count,
            digest: wire.digest,
            next: wire.next,
        })
    }
}
pub(crate) fn validate_effect_page(
    entries: &[DecisionEffect],
    header: &DecisionEffectsHeader,
    has_next: bool,
) -> Result<Option<bool>, TextValidationError> {
    if entries.len() > MAX_EFFECT_PAGE_ENTRIES
        || u64::try_from(entries.len())
            .ok()
            .is_none_or(|count| count > header.count)
        || (has_next && entries.len() != MAX_EFFECT_PAGE_ENTRIES)
    {
        return Err(invalid("effects"));
    }
    let project = entries
        .first()
        .map(|(_, position)| matches!(position, RecordPlacement::Project(_)));
    if entries.iter().any(|(_, position)| {
        !matches!(
            (project, position),
            (Some(true), RecordPlacement::Project(_)) | (Some(false), RecordPlacement::Fleet(_))
        )
    }) {
        return Err(invalid("effects"));
    }
    if has_next
        && u64::try_from(entries.len())
            .ok()
            .is_none_or(|count| count >= header.count)
    {
        return Err(invalid("next"));
    }
    if project == Some(true)
        && entries
            .iter()
            .zip(entries.iter().skip(1))
            .any(|(left, right)| {
                left.1.project_cursor().map(|cursor| cursor.position)
                    >= right.1.project_cursor().map(|cursor| cursor.position)
            })
    {
        return Err(invalid("effects"));
    }
    if !has_next
        && u64::try_from(entries.len()).ok() == Some(header.count)
        && !header.matches_full(entries)
    {
        return Err(invalid("effects"));
    }
    Ok(project)
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

impl StoredDecision {
    #[must_use]
    pub fn accepted(effects: Vec<DecisionEffect>, change: ChangeOutcome) -> Self {
        Self::Accepted {
            effects: DecisionEffectsHeader::from_full(&effects),
            change,
        }
    }
    pub fn into_reply(
        self,
        effects: Vec<DecisionEffect>,
        after: Option<&RequestEffectsCursor>,
    ) -> Result<RequestDecision, TextValidationError> {
        match self {
            Self::Accepted {
                effects: header,
                change,
            } => Ok(RequestDecision::Accepted {
                effects: DecisionEffectsPage::from_full(effects, &header, after)?,
                change,
            }),
            Self::Rejected { reasons } if after.is_none() => {
                Ok(RequestDecision::Rejected { reasons })
            }
            Self::Rejected { .. } => Err(invalid("after")),
        }
    }
}
