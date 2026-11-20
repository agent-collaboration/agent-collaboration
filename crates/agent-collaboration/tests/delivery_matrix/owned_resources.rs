//! Private actor/root ownership metadata for one matrix run; no service calls or cleanup
//! execution.
use collaboration_client::board::{Identity, ThreadId};
use std::fmt;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum OwnershipError {
    ActorRootLimitReached,
}
impl fmt::Display for OwnershipError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActorRootLimitReached => {
                formatter.write_str("matrix ownership is limited to four distinct actor/root pairs")
            }
        }
    }
}
impl std::error::Error for OwnershipError {}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct OwnedActorRoot {
    pub(super) actor: Identity,
    pub(super) root_message_id: ThreadId,
}

/// Actor/root pairs the run subscribed to or watched, retired as Unsubscribe then Unwatch.
#[derive(Debug, Default)]
pub(super) struct OwnedMatrixResources {
    actor_roots: Vec<OwnedActorRoot>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum CleanupIntent {
    Unsubscribe {
        actor: Identity,
        root_message_id: ThreadId,
    },
    Unwatch {
        actor: Identity,
        root_message_id: ThreadId,
    },
}

#[derive(Debug)]
pub(super) struct MatrixCreationHistory {
    pub(super) actor_roots: Vec<OwnedActorRoot>,
}

#[derive(Debug)]
pub(super) struct MatrixCleanupPlan {
    pub(super) intents: Vec<CleanupIntent>,
    pub(super) history: MatrixCreationHistory,
}

impl OwnedMatrixResources {
    pub(super) fn register_actor_root(
        &mut self,
        actor: Identity,
        root_message_id: ThreadId,
    ) -> Result<(), OwnershipError> {
        let pair = OwnedActorRoot {
            actor,
            root_message_id,
        };
        if self.actor_roots.contains(&pair) {
            return Ok(());
        }
        if self.actor_roots.len() >= 4 {
            return Err(OwnershipError::ActorRootLimitReached);
        }
        self.actor_roots.push(pair);
        Ok(())
    }

    pub(super) fn into_cleanup_plan(self) -> MatrixCleanupPlan {
        let mut intents = Vec::with_capacity(self.actor_roots.len() * 2);
        for pair in &self.actor_roots {
            intents.push(CleanupIntent::Unsubscribe {
                actor: pair.actor.clone(),
                root_message_id: pair.root_message_id.clone(),
            });
            intents.push(CleanupIntent::Unwatch {
                actor: pair.actor.clone(),
                root_message_id: pair.root_message_id.clone(),
            });
        }
        MatrixCleanupPlan {
            intents,
            history: MatrixCreationHistory {
                actor_roots: self.actor_roots,
            },
        }
    }
}

#[cfg(test)]
#[path = "owned_resources_tests.rs"]
mod tests;
