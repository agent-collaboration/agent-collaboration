use super::{
    effects::{ApplyFailure, EffectHandler, ObjectHostEffects},
    failures::FailureOutcome,
    source::{EventSource, SourcePolicy},
    store_adapter::{LoadFailure, StoreAdapter, Versioned},
};
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_engine_model::{
    Context, InstanceKey, ItemFailureClass, Machine, RefusalTag, TransitionError,
};
use central_storage::StoreFailure;

pub(crate) enum HandleOutcome<State, Error> {
    Done(State),
    Refused(TransitionError<Error>),
    NotFound,
    Failed(ItemFailureClass),
    /// A request whose subject or batch is broken gets a typed answer; only due
    /// units charge an attempt record (P2, R3).
    RequestFailed(ItemFailureClass),
    Superseded,
    Uncertain,
    StoreDown(StoreFailure),
}
impl<State, Error> From<FailureOutcome> for HandleOutcome<State, Error> {
    fn from(outcome: FailureOutcome) -> Self {
        match outcome {
            FailureOutcome::Recorded(record) => Self::Failed(record.last_failure),
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
    pub(crate) async fn handle<M, Adapter, Source>(
        &self,
        adapter: &Adapter,
        key: InstanceKey,
        event: M::Event,
        source: &Source,
    ) -> HandleOutcome<M::State, M::Error>
    where
        M: Machine,
        M::State: Send + Sync,
        M::Event: Send + Sync,
        M::Effect: Send + Sync,
        M::Error: Send + Sync,
        Adapter: StoreAdapter<M>,
        Source: EventSource<M>,
    {
        let handler = ObjectHostEffects {
            host: self.host.as_ref(),
            #[cfg(feature = "test-support")]
            pauses: &self.pauses,
        };
        for _ in 0..2 {
            let prior = match adapter.load(&key).await {
                Ok(Some(prior)) => prior,
                Ok(None) => {
                    if source.policy() != SourcePolicy::Request {
                        return HandleOutcome::NotFound;
                    }
                    let receipt = match source.missing() {
                        Ok(receipt) => receipt,
                        Err(failure) => return HandleOutcome::StoreDown(failure),
                    };
                    return match handler.apply_receipt(receipt).await {
                        Ok(()) => HandleOutcome::NotFound,
                        Err(failure) => receipt_failure(failure),
                    };
                }
                Err(LoadFailure::Corrupt { class, .. })
                    if source.policy() == SourcePolicy::Request =>
                {
                    return HandleOutcome::RequestFailed(class);
                }
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
            if source.policy() == SourcePolicy::Due
                && let Some(outcome) = self
                    .due_attempt_gate::<M, Adapter>(adapter, &key, &prior)
                    .await
            {
                return outcome;
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
                M::transition(
                    &prior.state,
                    event.clone(),
                    &mut Context { now, ids: &mut ids },
                )
            };
            let (next, effects) = match transition {
                Ok(outcome) => outcome,
                Err(error) => match source.policy() {
                    SourcePolicy::Due => {
                        let refusal = match error {
                            TransitionError::Stale => return HandleOutcome::Superseded,
                            TransitionError::NotAllowed { state, event } => {
                                format!("{state}:{event}")
                            }
                            TransitionError::Domain(error) => error.refusal_tag().to_owned(),
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
                    SourcePolicy::Discard => return HandleOutcome::Refused(error),
                    SourcePolicy::Request => {
                        let receipt = match source.refused(&error) {
                            Ok(receipt) => receipt,
                            Err(failure) => return HandleOutcome::StoreDown(failure),
                        };
                        return match handler.apply_receipt(receipt).await {
                            Ok(()) => HandleOutcome::Refused(error),
                            Err(failure) => receipt_failure(failure),
                        };
                    }
                },
            };
            let receipt = match source.accepted(&next) {
                Ok(receipt) => receipt,
                Err(failure) => return HandleOutcome::StoreDown(failure),
            };
            match handler
                .apply::<M, Adapter>(adapter, &key, &prior, &next, &effects, receipt)
                .await
            {
                Ok(()) => return HandleOutcome::Done(next),
                Err(ApplyFailure::Superseded) => {
                    if source.policy() == SourcePolicy::Discard {
                        return HandleOutcome::Superseded;
                    }
                }
                Err(ApplyFailure::Rejected(class)) if source.policy() == SourcePolicy::Request => {
                    return HandleOutcome::RequestFailed(class);
                }
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

    async fn due_attempt_gate<M: Machine, Adapter: StoreAdapter<M>>(
        &self,
        adapter: &Adapter,
        key: &InstanceKey,
        prior: &Versioned<M::State>,
    ) -> Option<HandleOutcome<M::State, M::Error>> {
        let subject = adapter.subject(key);
        match super::attempt_store::load_attempt(self.host.as_ref(), &subject).await {
            Ok(Some(stored)) => match &stored.record {
                Err(corruption) => Some(
                    self.write_failure(
                        &subject,
                        adapter.failure_sql(),
                        &prior.version,
                        corruption.failure_class(),
                        M::retirement_threshold(),
                        Some(&stored),
                    )
                    .await
                    .into(),
                ),
                Ok(record) => match record.state {
                    central_engine_model::AttemptState::Retired { .. } => {
                        Some(HandleOutcome::Superseded)
                    }
                    central_engine_model::AttemptState::Failing { next_try_at } => match self.now()
                    {
                        Ok(now) if next_try_at <= now => None,
                        Ok(_) => Some(HandleOutcome::Superseded),
                        Err(failure) => Some(HandleOutcome::StoreDown(failure)),
                    },
                },
            },
            Ok(None) => None,
            Err(StoreFailure::AnswerLost) => Some(HandleOutcome::Uncertain),
            Err(failure) => Some(HandleOutcome::StoreDown(failure)),
        }
    }
}
fn receipt_failure<State, Error>(failure: ApplyFailure) -> HandleOutcome<State, Error> {
    match failure {
        ApplyFailure::Superseded => HandleOutcome::Superseded,
        ApplyFailure::Uncertain => HandleOutcome::Uncertain,
        ApplyFailure::StoreDown(failure) => HandleOutcome::StoreDown(failure),
        ApplyFailure::Rejected(_) => {
            HandleOutcome::StoreDown(crate::workspace_decode_error::corrupt("request_log.claim"))
        }
    }
}
