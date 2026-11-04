//! Conversation-seat refusals preserve their evidence without becoming authority roles.
use crate::{FeedCursor, Identity, ThreadId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! thread_refusal_shapes {
    ($($kind:ident { $($field:ident: $value:ty),* $(,)? }),* $(,)?) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
        #[serde(tag="kind", rename_all="camelCase", rename_all_fields="camelCase", deny_unknown_fields)]
        pub enum ThreadRefusal { $($kind { $($field: $value),* }),* }

        #[derive(Deserialize)]
        #[serde(tag="kind", rename_all="camelCase", rename_all_fields="camelCase", deny_unknown_fields)]
        enum ThreadRefusalWire { $($kind { $($field: $value),* }),* }

        impl<'de> Deserialize<'de> for ThreadRefusal {
            fn deserialize<Deserializer: serde::Deserializer<'de>>(deserializer: Deserializer) -> Result<Self, Deserializer::Error> {
                let refusal = match ThreadRefusalWire::deserialize(deserializer)? {
                    $(ThreadRefusalWire::$kind { $($field),* } => Self::$kind { $($field),* }),*
                };
                refusal.validate().map_err(serde::de::Error::custom)?;
                Ok(refusal)
            }
        }
    };
}

thread_refusal_shapes! {
    ParticipantRequired { thread_id: ThreadId, actor: Identity },
    OrchestratorRequired { thread_id: ThreadId, actor: Identity, holder: Option<Identity>, last_seen: Option<FeedCursor> },
    OrchestratorAlreadyExists { thread_id: ThreadId, actor: Identity, holder: Identity, last_seen: FeedCursor },
    ImplementerAlreadyExists { thread_id: ThreadId, actor: Identity, holder: Identity, last_seen: FeedCursor },
    OrchestratorHandoverRequired { thread_id: ThreadId, actor: Identity },
    HandoverTargetNotParticipant { thread_id: ThreadId, actor: Identity, target: Identity },
    StaleOrchestrator { thread_id: ThreadId, actor: Identity, holder: Option<Identity>, last_seen: Option<FeedCursor>, named_holder: Identity },
    StaleImplementer { thread_id: ThreadId, actor: Identity, holder: Option<Identity>, last_seen: Option<FeedCursor>, named_holder: Identity },
    SelfReplace { thread_id: ThreadId, actor: Identity },
    OrchestratorRoleChange { thread_id: ThreadId, actor: Identity },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("lastSeen must describe the current holder, or both must be absent for a vacant seat")]
pub struct ThreadRefusalValidationError;

impl ThreadRefusal {
    /// A stale-seat cursor describes its current holder; other variants keep their own rules.
    pub fn validate(&self) -> Result<(), ThreadRefusalValidationError> {
        if let Self::StaleOrchestrator {
            holder, last_seen, ..
        }
        | Self::StaleImplementer {
            holder, last_seen, ..
        } = self
            && holder.is_some() != last_seen.is_some()
        {
            return Err(ThreadRefusalValidationError);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "thread_refusals_test.rs"]
mod tests;
