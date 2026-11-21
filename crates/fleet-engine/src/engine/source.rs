#[cfg(feature = "test-support")]
use super::receipts::{EngineReceipt, EngineRefusal, ReceiptClaim};
use central_engine_model::{Machine, TransitionError};
use central_storage::{GuardedStatement, StoreFailure};
#[cfg(feature = "test-support")]
use serde::Serialize;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SourcePolicy {
    Due,
    Request,
    Discard,
}

pub(crate) enum ReceiptStatements {
    None,
    #[cfg_attr(
        not(feature = "test-support"),
        expect(
            dead_code,
            reason = "only explicit machine requests carry these receipts"
        )
    )]
    Request(Vec<GuardedStatement>),
    Admin(Vec<GuardedStatement>),
}
impl ReceiptStatements {
    pub fn into_statements(self) -> Vec<GuardedStatement> {
        match self {
            Self::None => vec![],
            Self::Request(statements) | Self::Admin(statements) => statements,
        }
    }
}

/// Static source-specific receipt construction keeps serialization off the
/// Machine contract. A due event has no receipt; request states opt into Serde.
pub(crate) trait EventSource<M: Machine>: Sync {
    fn policy(&self) -> SourcePolicy;
    fn accepted(&self, _state: &M::State) -> Result<ReceiptStatements, StoreFailure> {
        Ok(ReceiptStatements::None)
    }
    fn refused(
        &self,
        _error: &TransitionError<M::Error>,
    ) -> Result<ReceiptStatements, StoreFailure> {
        Ok(ReceiptStatements::None)
    }
    fn missing(&self) -> Result<ReceiptStatements, StoreFailure> {
        Ok(ReceiptStatements::None)
    }
}

pub(crate) struct DueSource;
impl<M: Machine> EventSource<M> for DueSource {
    fn policy(&self) -> SourcePolicy {
        SourcePolicy::Due
    }
}

#[cfg(feature = "test-support")]
pub(crate) struct RequestSource(pub ReceiptClaim);
#[cfg(feature = "test-support")]
impl<M: Machine> EventSource<M> for RequestSource
where
    M::State: Serialize,
{
    fn policy(&self) -> SourcePolicy {
        SourcePolicy::Request
    }
    fn accepted(&self, state: &M::State) -> Result<ReceiptStatements, StoreFailure> {
        self.0
            .statements(&EngineReceipt::<_, M::Error>::Applied { state })
            .map(ReceiptStatements::Request)
    }
    fn refused(
        &self,
        error: &TransitionError<M::Error>,
    ) -> Result<ReceiptStatements, StoreFailure> {
        let error = match error {
            TransitionError::Domain(error) => EngineRefusal::Domain { error },
            TransitionError::NotAllowed { state, event } => EngineRefusal::NotAllowed {
                state: (*state).into(),
                event: (*event).into(),
            },
            TransitionError::Stale => EngineRefusal::Stale,
        };
        self.0
            .statements(&EngineReceipt::<M::State, _>::Refused { error })
            .map(ReceiptStatements::Request)
    }
    fn missing(&self) -> Result<ReceiptStatements, StoreFailure> {
        self.0
            .statements(&EngineReceipt::<M::State, M::Error>::NotFound)
            .map(ReceiptStatements::Request)
    }
}
