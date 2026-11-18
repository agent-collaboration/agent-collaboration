use super::port::{Engine, EngineHost};
use super::{
    attempt_records::EngineSubject,
    handle::HandleOutcome,
    receipts::{EngineReceipt, ReceiptClaim},
    source::RequestSource,
    store_adapter::StoreAdapter,
};
use crate::workspace_decode_error::corrupt;
use central_engine_model::{InstanceKey, Machine};
use central_storage::StoreFailure;
use fleet_runtime_ports::CentralStore as _;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use workspace_model::{ActorIdentityId, RequestId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase", deny_unknown_fields)]
pub enum EngineRequestOutcome<State, Error> {
    Answer {
        receipt: EngineReceipt<State, Error>,
    },
    Conflict,
    OutcomeUnknown,
    ItemFailed {
        failure: central_engine_model::ItemFailureClass,
    },
    CentralUnavailable,
    NotPermitted {
        reason: workspace_model::RefusalReason,
    },
}

impl<R: EngineHost> Engine<'_, R> {
    pub(crate) async fn request_engine<M, Adapter>(
        &self,
        adapter: &Adapter,
        caller: ActorIdentityId,
        request: RequestId,
        key: InstanceKey,
        event: M::Event,
    ) -> Result<EngineRequestOutcome<M::State, M::Error>, StoreFailure>
    where
        M: Machine,
        M::State: Send + Sync + Serialize + DeserializeOwned,
        M::Event: Send + Sync + Serialize,
        M::Effect: Send + Sync,
        M::Error: Send + Sync,
        Adapter: StoreAdapter<M>,
    {
        let answer = self
            .execute_engine_request::<M, Adapter>(adapter, caller, request, key, event)
            .await;
        let _ = self.after_event().await;
        answer
    }

    async fn execute_engine_request<M, Adapter>(
        &self,
        adapter: &Adapter,
        caller: ActorIdentityId,
        request: RequestId,
        key: InstanceKey,
        event: M::Event,
    ) -> Result<EngineRequestOutcome<M::State, M::Error>, StoreFailure>
    where
        M: Machine,
        M::State: Send + Sync + Serialize + DeserializeOwned,
        M::Event: Send + Sync + Serialize,
        M::Effect: Send + Sync,
        M::Error: Send + Sync,
        Adapter: StoreAdapter<M>,
    {
        // The request entry (for example `request_probe`) checked the organization
        // once for this event, before any read (P3).
        let now = self.now()?;
        let cutoff = now
            .checked_sub_signed(chrono::Duration::hours(24))
            .ok_or_else(|| corrupt("request_log.cutoff"))?;
        let digest = crate::workspace_request_log::digest(
            &serde_json::json!({"machine":M::KIND.0,"key":&key.0,"event":&event}),
        )?;
        let source = RequestSource(ReceiptClaim {
            request,
            caller,
            digest,
            started_at: now,
            cutoff,
        });
        let logged = self.host().read(source.0.lookup()).await?;
        if let Some(receipt) = source.0.replay(&logged)? {
            return Ok(EngineRequestOutcome::Answer { receipt });
        }
        #[cfg(feature = "test-support")]
        self.pauses()
            .reached_engine(
                crate::PausePoint::AfterEngineAbsentLookup,
                &source.0.request.to_string(),
            )
            .await;
        let subject = EngineSubject {
            kind: M::KIND,
            key: key.clone(),
        };
        Ok(
            match self
                .handle::<M, Adapter, _>(adapter, key, event, &source)
                .await
            {
                HandleOutcome::Done(state) => EngineRequestOutcome::Answer {
                    receipt: EngineReceipt::Applied { state },
                },
                HandleOutcome::Refused(error) => EngineRequestOutcome::Answer {
                    receipt: EngineReceipt::Refused {
                        error: error.into(),
                    },
                },
                HandleOutcome::NotFound => EngineRequestOutcome::Answer {
                    receipt: EngineReceipt::NotFound,
                },
                HandleOutcome::Failed(failure) | HandleOutcome::RequestFailed(failure) => {
                    EngineRequestOutcome::ItemFailed { failure }
                }
                HandleOutcome::Uncertain => EngineRequestOutcome::OutcomeUnknown,
                HandleOutcome::StoreDown(failure) => {
                    self.observe_engine_store_down(&subject, failure);
                    EngineRequestOutcome::CentralUnavailable
                }
                HandleOutcome::Superseded => {
                    let logged = self.host().read(source.0.lookup()).await?;
                    match source.0.replay(&logged)? {
                        Some(receipt) => EngineRequestOutcome::Answer { receipt },
                        None => EngineRequestOutcome::Conflict,
                    }
                }
            },
        )
    }
}
