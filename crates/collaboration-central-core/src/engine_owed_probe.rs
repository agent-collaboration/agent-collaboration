//! Test-support entry that runs the owed-reply reducer through `handle` on a
//! real host (P10, V1). No production route or table reaches it.
pub use crate::engine::machines::owed_reply::{
    Event as OwedEvent, Finish as OwedFinish, OwedReply, OwedReplyRefusal,
    RouterAnswer as OwedAnswer, State as OwedState,
};
pub use crate::engine::{
    receipts::{EngineReceipt, EngineRefusal},
    requests::EngineRequestOutcome,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedOwedReply {
    pub key: uuid::Uuid,
    pub group: uuid::Uuid,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwedReplyRequest {
    pub request_id: workspace_model::RequestId,
    pub key: central_engine_model::InstanceKey,
    pub event: OwedEvent,
}

impl<Host: crate::ObjectHost, Stores: crate::WorkspaceStoreCalls>
    crate::WorkspaceRuntime<Host, Stores>
{
    pub async fn seed_owed_reply(
        &self,
        seed: SeedOwedReply,
    ) -> Result<central_storage::BatchOutcome, central_storage::StoreFailure> {
        use crate::{
            central_mail_store::timestamp,
            guarded_statements::{any, exact, text},
            workspace_decode_error::corrupt,
        };
        if !self.alarm_organization_is_provisioned().await? {
            return Err(corrupt("signin_organization_identity.id"));
        }
        if seed.key.get_version_num() != 7 || seed.group.get_version_num() != 7 {
            return Err(corrupt("engine_probe_items.id"));
        }
        let now = timestamp(self.now()?);
        self.host.run_batch(central_storage::StatementBatch {
            statements: vec![
                any("INSERT INTO engine_probe_groups(group_id,effect_count,created_at,updated_at) VALUES (?,0,?,?) ON CONFLICT(group_id) DO NOTHING",vec![text(seed.group),text(&now),text(&now)]),
                exact("INSERT INTO engine_probe_items(item_id,machine_kind,group_id,due_at,claim,state,completed_claim,send_id,connection_id,created_at,updated_at) VALUES (?,'owed_reply',?,NULL,?,'waiting',NULL,NULL,NULL,?,?)",
                    vec![text(seed.key),text(seed.group),text(format!("owed:{}", seed.key)),text(&now),text(&now)]),
            ],
            alarm: central_storage::AlarmUpdate::Unchanged,
        }).await
    }

    pub async fn request_owed_reply(
        &self,
        caller: &crate::WorkspaceCaller,
        request: OwedReplyRequest,
    ) -> Result<EngineRequestOutcome<OwedState, OwedReplyRefusal>, central_storage::StoreFailure>
    {
        if !self.alarm_organization_is_provisioned().await? {
            return Err(crate::workspace_decode_error::corrupt(
                "signin_organization_identity.id",
            ));
        }
        if let Err(reason) = self.admit_workspace(caller, None).await? {
            return Ok(EngineRequestOutcome::NotPermitted { reason });
        }
        let identity = match caller.caller() {
            workspace_model::Caller::Session(session) => workspace_model::Identity::AgentSession {
                session: session.clone(),
            },
            workspace_model::Caller::Person { person_id, .. } => {
                workspace_model::Identity::Person {
                    person_id: person_id.clone(),
                }
            }
        };
        let actor = self.resolve_identity(&identity).await?;
        crate::engine::port::Engine::new(self)
            .request_engine::<OwedReply, _>(
                &crate::engine::owed_reply_store::OwedReplyStore(self),
                actor,
                request.request_id,
                request.key,
                request.event,
            )
            .await
    }
}
