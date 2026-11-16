use super::{
    effects::{ApplyFailure, EffectHandler, ObjectHostEffects},
    failures::FailureOutcome,
    store_adapter::{LoadFailure, StoreAdapter},
};
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_engine_model::{
    Context, InstanceKey, ItemFailureClass, Machine, RefusalTag, TransitionError,
};
use central_storage::StoreFailure;

pub(crate) enum HandleOutcome<State> {
    Done(State),
    Failed,
    Superseded,
    Uncertain,
    StoreDown(StoreFailure),
}

impl<State> From<FailureOutcome> for HandleOutcome<State> {
    fn from(outcome: FailureOutcome) -> Self {
        match outcome {
            FailureOutcome::Recorded(_) => Self::Failed,
            FailureOutcome::Superseded => Self::Superseded,
            FailureOutcome::Uncertain => Self::Uncertain,
            FailureOutcome::StoreDown(failure) => Self::StoreDown(failure),
        }
    }
}

struct ReducerIds<'a>(&'a dyn runtime_primitives::IdSource);
impl central_engine_model::IdSource for ReducerIds<'_> {
    fn next_id(&mut self) -> uuid::Uuid {
        self.0.new_id()
    }
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn handle<M, Adapter>(
        &self,
        adapter: &Adapter,
        key: InstanceKey,
        event: M::Event,
    ) -> HandleOutcome<M::State>
    where
        M: Machine,
        M::State: Sync,
        M::Effect: Sync,
        Adapter: StoreAdapter<M>,
    {
        match self.alarm_organization_is_provisioned().await {
            Ok(true) => {}
            Ok(false) => return HandleOutcome::Superseded,
            Err(failure) => return HandleOutcome::StoreDown(failure),
        }
        for _ in 0..2 {
            let prior = match adapter.load(&key).await {
                Ok(Some(prior)) => prior,
                Ok(None) => return HandleOutcome::Superseded,
                Err(LoadFailure::Corrupt { class, version }) => {
                    return self
                        .record_failure(
                            &adapter.subject(&key),
                            adapter.failure_sql(),
                            &version,
                            class,
                            M::retirement_threshold(),
                        )
                        .await
                        .into();
                }
                Err(LoadFailure::Uncertain) => return HandleOutcome::Uncertain,
                Err(LoadFailure::StoreDown(failure)) => return HandleOutcome::StoreDown(failure),
            };
            let subject = adapter.subject(&key);
            match super::attempt_store::load_attempt(self.host.as_ref(), &subject).await {
                Ok(Some(stored)) => match &stored.record {
                    Err(corruption) => {
                        return self
                            .write_failure(
                                &subject,
                                adapter.failure_sql(),
                                &prior.version,
                                corruption.failure_class(),
                                M::retirement_threshold(),
                                Some(&stored),
                            )
                            .await
                            .into();
                    }
                    Ok(record) => match record.state {
                        central_engine_model::AttemptState::Retired { .. } => {
                            return HandleOutcome::Superseded;
                        }
                        central_engine_model::AttemptState::Failing { next_try_at } => {
                            match self.now() {
                                Ok(now) if next_try_at <= now => {}
                                Ok(_) => return HandleOutcome::Superseded,
                                Err(failure) => return HandleOutcome::StoreDown(failure),
                            }
                        }
                    },
                },
                Ok(None) => {}
                Err(StoreFailure::AnswerLost) => return HandleOutcome::Uncertain,
                Err(failure) => return HandleOutcome::StoreDown(failure),
            }
            #[cfg(feature = "test-support")]
            self.pauses
                .reached_central(crate::PausePoint::AfterEngineLoad)
                .await;
            let now = match self.now() {
                Ok(now) => now,
                Err(failure) => return HandleOutcome::StoreDown(failure),
            };
            let transition = {
                let mut ids = ReducerIds(self.ids.as_ref());
                let mut context = Context { now, ids: &mut ids };
                M::transition(&prior.state, event.clone(), &mut context)
            };
            let (next, effects) = match transition {
                Ok(outcome) => outcome,
                Err(TransitionError::Stale) => return HandleOutcome::Superseded,
                Err(error) => {
                    let refusal = match error {
                        TransitionError::NotAllowed { state, event } => format!("{state}:{event}"),
                        TransitionError::Domain(error) => error.refusal_tag().to_owned(),
                        TransitionError::Stale => unreachable!("handled above"),
                    };
                    return self
                        .record_failure(
                            &adapter.subject(&key),
                            adapter.failure_sql(),
                            &prior.version,
                            ItemFailureClass::Refused { refusal },
                            M::retirement_threshold(),
                        )
                        .await
                        .into();
                }
            };
            let handler = ObjectHostEffects {
                host: self.host.as_ref(),
                #[cfg(feature = "test-support")]
                pauses: &self.pauses,
            };
            match handler
                .apply::<M, Adapter>(adapter, &key, &prior, &next, &effects, vec![])
                .await
            {
                Ok(()) => return HandleOutcome::Done(next),
                Err(ApplyFailure::Superseded) => continue,
                Err(ApplyFailure::Rejected(class)) => {
                    return self
                        .record_failure(
                            &adapter.subject(&key),
                            adapter.failure_sql(),
                            &prior.version,
                            class,
                            M::retirement_threshold(),
                        )
                        .await
                        .into();
                }
                Err(ApplyFailure::Uncertain) => return HandleOutcome::Uncertain,
                Err(ApplyFailure::StoreDown(failure)) => return HandleOutcome::StoreDown(failure),
            }
        }
        HandleOutcome::Superseded
    }
}
