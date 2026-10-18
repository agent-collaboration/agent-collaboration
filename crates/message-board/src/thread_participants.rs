use crate::{FeedCursor, Identity};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const MAX_PARTICIPANT_NOTE_BYTES: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ParticipantRole {
    Orchestrator,
    Implementer,
    Advisor,
    Reviewer,
    Participant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ParticipantClosedReason {
    Left,
    Replaced,
    Resolved,
}

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("participant note must contain 1 to 16384 trimmed UTF-8 bytes without NUL")]
pub struct InvalidParticipantNote;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ParticipantNote(String);

impl TryFrom<String> for ParticipantNote {
    type Error = InvalidParticipantNote;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let trimmed = value.trim();
        if trimmed.is_empty()
            || trimmed.len() > MAX_PARTICIPANT_NOTE_BYTES
            || trimmed.contains('\0')
        {
            return Err(InvalidParticipantNote);
        }
        Ok(Self(trimmed.to_owned()))
    }
}

impl From<ParticipantNote> for String {
    fn from(note: ParticipantNote) -> Self {
        note.0
    }
}

impl ParticipantNote {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ParticipantNote {
    fn schema_name() -> Cow<'static, str> {
        "ParticipantNote".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({"type":"string","minLength":1,"maxLength":16384})
    }
}

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("participant open and closed fields do not form a valid Participant state")]
pub struct InvalidParticipantState;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Participant {
    pub identity: Identity,
    pub role: ParticipantRole,
    pub note: Option<ParticipantNote>,
    pub joined_at_position: FeedCursor,
    pub last_seen_position: FeedCursor,
    pub closed_at_position: Option<FeedCursor>,
    pub closed_reason: Option<ParticipantClosedReason>,
    pub replaced_by: Option<Identity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantProps {
    pub identity: Identity,
    pub role: ParticipantRole,
    pub note: Option<ParticipantNote>,
    pub joined_at_position: FeedCursor,
    pub last_seen_position: FeedCursor,
    pub closed_at_position: Option<FeedCursor>,
    pub closed_reason: Option<ParticipantClosedReason>,
    pub replaced_by: Option<Identity>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParticipantWire {
    identity: Identity,
    role: ParticipantRole,
    note: Option<ParticipantNote>,
    joined_at_position: FeedCursor,
    last_seen_position: FeedCursor,
    closed_at_position: Option<FeedCursor>,
    closed_reason: Option<ParticipantClosedReason>,
    replaced_by: Option<Identity>,
}

impl<'de> Deserialize<'de> for Participant {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ParticipantWire::deserialize(deserializer)?;
        Self::new(ParticipantProps {
            identity: wire.identity,
            role: wire.role,
            note: wire.note,
            joined_at_position: wire.joined_at_position,
            last_seen_position: wire.last_seen_position,
            closed_at_position: wire.closed_at_position,
            closed_reason: wire.closed_reason,
            replaced_by: wire.replaced_by,
        })
        .map_err(serde::de::Error::custom)
    }
}

impl Participant {
    pub fn new(props: ParticipantProps) -> Result<Self, InvalidParticipantState> {
        if props.last_seen_position.position < props.joined_at_position.position
            || props.last_seen_position.epoch < props.joined_at_position.epoch
        {
            return Err(InvalidParticipantState);
        }
        match (
            props.closed_at_position,
            props.closed_reason,
            props.replaced_by.as_ref(),
        ) {
            (None, None, None) => {}
            (Some(closed), Some(ParticipantClosedReason::Left), None)
            | (Some(closed), Some(ParticipantClosedReason::Resolved), None)
                if closed == props.last_seen_position => {}
            (Some(closed), Some(ParticipantClosedReason::Replaced), Some(replacement))
                if matches!(
                    props.role,
                    ParticipantRole::Orchestrator | ParticipantRole::Implementer
                ) && closed == props.last_seen_position
                    && *replacement != props.identity => {}
            _ => return Err(InvalidParticipantState),
        }
        Ok(Self {
            identity: props.identity,
            role: props.role,
            note: props.note,
            joined_at_position: props.joined_at_position,
            last_seen_position: props.last_seen_position,
            closed_at_position: props.closed_at_position,
            closed_reason: props.closed_reason,
            replaced_by: props.replaced_by,
        })
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.closed_at_position.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrchestratorHolder {
    pub identity: Identity,
    pub last_seen_position: FeedCursor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImplementerHolder {
    pub identity: Identity,
    pub last_seen_position: FeedCursor,
}
