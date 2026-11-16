use super::{
    attempt_statements::delete_attempt,
    store_adapter::{StoreAdapter, Versioned},
};
use crate::ObjectHost;
use central_engine_model::{InstanceKey, ItemFailureClass, Machine};
use central_storage::{AlarmUpdate, BatchOutcome, GuardedStatement, StatementBatch, StoreFailure};
use std::future::Future;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ApplyFailure {
    Superseded,
    Rejected(ItemFailureClass),
    Uncertain,
    StoreDown(StoreFailure),
}

pub(crate) fn classify_batch(
    result: Result<BatchOutcome, StoreFailure>,
) -> Result<(), ApplyFailure> {
    match result {
        Ok(BatchOutcome::Applied { .. }) => Ok(()),
        Ok(BatchOutcome::GuardFailed { .. }) => Err(ApplyFailure::Superseded),
        Ok(BatchOutcome::ConstraintViolated { kind, .. }) => Err(ApplyFailure::Rejected(
            ItemFailureClass::ConstraintViolated { constraint: kind },
        )),
        Ok(BatchOutcome::StatementRejected { .. }) => {
            Err(ApplyFailure::Rejected(ItemFailureClass::StatementRejected))
        }
        Err(StoreFailure::AnswerLost) => Err(ApplyFailure::Uncertain),
        Err(failure @ (StoreFailure::Unavailable | StoreFailure::AlarmUnpublishable { .. })) => {
            Err(ApplyFailure::StoreDown(failure))
        }
        Err(StoreFailure::Corrupt { reason }) => Err(ApplyFailure::Rejected(
            super::store_adapter::corrupt_class(reason),
        )),
    }
}

/// The ObjectHost supplies the platform transaction; the adapter supplies the
/// machine's statements. Both modern hosts use this same effect handler.
pub(crate) trait EffectHandler {
    fn apply<M, Adapter>(
        &self,
        adapter: &Adapter,
        key: &InstanceKey,
        prior: &Versioned<M::State>,
        next: &M::State,
        effects: &[M::Effect],
        receipt: Vec<GuardedStatement>,
    ) -> impl Future<Output = Result<(), ApplyFailure>> + Send
    where
        M: Machine,
        M::State: Sync,
        M::Effect: Sync,
        Adapter: StoreAdapter<M>;
}

pub(crate) struct ObjectHostEffects<'a, Host: ObjectHost> {
    pub host: &'a Host,
    #[cfg(feature = "test-support")]
    pub pauses: &'a crate::PausePoints,
}

impl<Host: ObjectHost> EffectHandler for ObjectHostEffects<'_, Host> {
    async fn apply<M, Adapter>(
        &self,
        adapter: &Adapter,
        key: &InstanceKey,
        prior: &Versioned<M::State>,
        next: &M::State,
        effects: &[M::Effect],
        mut receipt: Vec<GuardedStatement>,
    ) -> Result<(), ApplyFailure>
    where
        M: Machine,
        M::State: Sync,
        M::Effect: Sync,
        Adapter: StoreAdapter<M>,
    {
        let statements = (|| {
            receipt.push(adapter.save(key, prior, next)?);
            for effect in effects {
                receipt.extend(adapter.effect_statements(key, effect)?);
            }
            receipt.push(delete_attempt(&adapter.subject(key)));
            Ok::<_, StoreFailure>(receipt)
        })()
        .map_err(|failure| match failure {
            StoreFailure::Corrupt { reason } => {
                ApplyFailure::Rejected(super::store_adapter::corrupt_class(reason))
            }
            StoreFailure::AnswerLost => ApplyFailure::Uncertain,
            failure @ (StoreFailure::Unavailable | StoreFailure::AlarmUnpublishable { .. }) => {
                ApplyFailure::StoreDown(failure)
            }
        })?;
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::BeforeTimerCommit)
            .await;
        classify_batch(
            self.host
                .run_batch(StatementBatch {
                    statements,
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        )?;
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::AfterTimerCommit)
            .await;
        Ok(())
    }
}
