//! The Host prepares a bound helper before the carrier consumes its one-shot operation.
use collaboration_client::ConversationCreateInput;
use collaboration_contracts::HarnessLaunchFailure;
use collaboration_service::fleet_session_registrar::BoundFleetSession;
use std::{future::Future, pin::Pin};

#[derive(Clone, Copy)]
pub struct BoundConversationPrepared;

pub type BoundConversationPreparationFuture<'a> = Pin<
    Box<dyn Future<Output = Result<BoundConversationPrepared, HarnessLaunchFailure>> + Send + 'a>,
>;

pub trait BoundConversationPreparation: Send + Sync {
    fn prepare<'a>(
        &'a self,
        caller: &'a BoundFleetSession,
        declared: &'a ConversationCreateInput,
    ) -> BoundConversationPreparationFuture<'a>;
}
