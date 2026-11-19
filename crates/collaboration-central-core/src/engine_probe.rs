//! Test-support entry for the probe machine on a real host (P9). No production
//! route or table reaches it; the machine itself lives in engine::probe_machine.
pub use crate::engine::probe_machine::{
    EngineProbe, ProbeEffect, ProbeEvent, ProbeRefusal, ProbeState,
};
pub use crate::engine::{
    receipts::{EngineReceipt, EngineRefusal},
    requests::EngineRequestOutcome,
};
use central_engine_model::EngineTime;
pub use central_engine_model::{InstanceKey, ItemFailureClass};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedProbe {
    pub key: uuid::Uuid,
    pub group: uuid::Uuid,
    pub due: Option<EngineTime>,
    pub claim: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeRequest {
    pub request_id: workspace_model::RequestId,
    pub key: central_engine_model::InstanceKey,
    pub event: ProbeEvent,
}

impl<Host: fleet_runtime_ports::CentralStore, Stores: fleet_runtime_ports::WorkspaceStores>
    crate::WorkspaceRuntime<Host, Stores>
{
    pub async fn seed_probe(
        &self,
        seed: SeedProbe,
    ) -> Result<central_storage::BatchOutcome, central_storage::StoreFailure> {
        use crate::{central_mail_store::timestamp, guarded_statements::exact};
        use fleet_runtime_ports::guarded_statements::{any, text};
        use fleet_runtime_ports::workspace_decode_error::corrupt;
        if !self.alarm_organization_is_provisioned().await? {
            return Err(corrupt("organization_identity.id"));
        }
        if seed.key.get_version_num() != 7 || seed.group.get_version_num() != 7 {
            return Err(corrupt("engine_probe_items.id"));
        }
        if seed.claim.is_empty()
            || seed
                .due
                .is_some_and(|time| !central_engine_model::supported_time(time))
        {
            return Err(corrupt("engine_probe_items.seed"));
        }
        let now = timestamp(self.fleet_runtime.now()?);
        self.statement_host().run_batch(central_storage::StatementBatch {
            statements:vec![
                any("INSERT INTO engine_probe_groups(group_id,effect_count,created_at,updated_at) VALUES (?,0,?,?) ON CONFLICT(group_id) DO NOTHING",vec![text(seed.group),text(&now),text(&now)]),
                exact("INSERT INTO engine_probe_items(item_id,machine_kind,group_id,due_at,claim,state,completed_claim,send_id,connection_id,created_at,updated_at) VALUES (?,'engine_probe',?,?,?,'waiting',NULL,NULL,NULL,?,?)",
                    vec![text(seed.key),text(seed.group),seed.due.map_or(central_storage::SqlValue::Null,|due|text(timestamp(due))),text(seed.claim),text(&now),text(&now)]),
            ],alarm:central_storage::AlarmUpdate::Unchanged,
        }).await
    }

    pub async fn request_probe(
        &self,
        caller: &fleet_contracts::WorkspaceCaller,
        request: ProbeRequest,
    ) -> Result<EngineRequestOutcome<ProbeState, ProbeRefusal>, central_storage::StoreFailure> {
        if !self.alarm_organization_is_provisioned().await? {
            return Err(fleet_runtime_ports::workspace_decode_error::corrupt(
                "organization_identity.id",
            ));
        }
        if let Err(reason) = self
            .workspace_access()
            .admit_workspace(caller, None)
            .await?
        {
            return Ok(EngineRequestOutcome::NotPermitted { reason });
        }
        let identity = match &caller.caller {
            workspace_model::Caller::Session(session) => workspace_model::Identity::AgentSession {
                session: session.clone(),
            },
            workspace_model::Caller::Person { person_id, .. } => {
                workspace_model::Identity::Person {
                    person_id: person_id.clone(),
                }
            }
        };
        let actor = self.workspace_access().resolve_identity(&identity).await?;
        crate::engine::port::Engine::new(self)
            .request_engine::<EngineProbe, _>(
                &crate::engine::probe_store::ProbeStore(self),
                actor,
                request.request_id,
                request.key,
                request.event,
            )
            .await
    }
}
