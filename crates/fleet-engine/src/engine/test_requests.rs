use super::port::{Engine, EngineHost};
use super::requests::EngineRequestOutcome;
use central_engine_model::InstanceKey;
use central_storage::StoreFailure;
use workspace_model::{ActorIdentityId, RequestId};

impl<Host: EngineHost> Engine<'_, Host> {
    pub async fn request_probe(
        &self,
        actor: ActorIdentityId,
        request_id: RequestId,
        key: InstanceKey,
        event: super::probe_machine::ProbeEvent,
    ) -> Result<
        EngineRequestOutcome<super::probe_machine::ProbeState, super::probe_machine::ProbeRefusal>,
        StoreFailure,
    > {
        self.request_engine::<super::probe_machine::EngineProbe, _>(
            &super::probe_store::ProbeStore(self.0),
            actor,
            request_id,
            key,
            event,
        )
        .await
    }
}

impl<Host: EngineHost> Engine<'_, Host> {
    pub async fn request_owed_reply(
        &self,
        actor: ActorIdentityId,
        request_id: RequestId,
        key: InstanceKey,
        event: super::machines::owed_reply::Event,
    ) -> Result<
        EngineRequestOutcome<
            super::machines::owed_reply::State,
            super::machines::owed_reply::OwedReplyRefusal,
        >,
        StoreFailure,
    > {
        self.request_engine::<super::machines::owed_reply::OwedReply, _>(
            &super::owed_reply_store::OwedReplyStore(self.0),
            actor,
            request_id,
            key,
            event,
        )
        .await
    }
}
