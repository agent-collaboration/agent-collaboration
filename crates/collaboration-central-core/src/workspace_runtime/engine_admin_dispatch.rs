//! Core's closed admin kind registry and discard routing.
use crate::engine::machines::{
    finish_store::{automation_finish_store, wake_finish_store},
    lifecycle_finish::FinishEvent,
};
use crate::{WorkspaceRuntime, organization_admin_contract::EngineKindName};
use central_storage::{GuardedStatement, StoreFailure};
use fleet_engine::{AttemptRefusal, DiscardOutcome, Engine, KindRegistry};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

/// Every kind health lists, zeros included. `engine_source` rows are the due-list
/// control subjects; the notice units are whole units, not machines.
const ENGINE_KINDS: &[(&str, EngineKindName)] = &[
    ("wake_timer", EngineKindName::WakeTimer),
    ("automation_timer", EngineKindName::AutomationTimer),
    ("direct_message", EngineKindName::DirectMessage),
    ("system_message_expiry", EngineKindName::SystemMessageExpiry),
    ("legacy_notice_windows", EngineKindName::LegacyNoticeWindows),
    (
        "legacy_workspace_drains",
        EngineKindName::LegacyWorkspaceDrains,
    ),
    ("engine_source", EngineKindName::EngineSource),
];
/// Test machines' records exist only in test-support stores and are not shown.
#[cfg(feature = "test-support")]
const HIDDEN_KINDS: &[&str] = &["engine_probe", "owed_reply"];
#[cfg(not(feature = "test-support"))]
const HIDDEN_KINDS: &[&str] = &[];

pub(crate) const KIND_REGISTRY: KindRegistry<'static, EngineKindName> = KindRegistry {
    shown: ENGINE_KINDS,
    hidden: HIDDEN_KINDS,
};

impl<H: CentralStore, S: WorkspaceStores> WorkspaceRuntime<H, S> {
    pub(crate) async fn engine_discard(
        &self,
        attempt_id: uuid::Uuid,
        statements: Vec<GuardedStatement>,
    ) -> Result<DiscardOutcome, StoreFailure> {
        let prepared = match Engine::new(self)
            .prepare_discard(&KIND_REGISTRY, attempt_id)
            .await?
        {
            Ok(prepared) => prepared,
            Err(refusal) => return Ok(DiscardOutcome::Refused(refusal)),
        };
        match prepared.kind() {
            EngineKindName::WakeTimer => {
                prepared
                    .finish(wake_finish_store(), FinishEvent::Discard, statements)
                    .await
            }
            EngineKindName::AutomationTimer => {
                prepared
                    .finish(automation_finish_store(), FinishEvent::Discard, statements)
                    .await
            }
            EngineKindName::DirectMessage
            | EngineKindName::SystemMessageExpiry
            | EngineKindName::LegacyNoticeWindows
            | EngineKindName::LegacyWorkspaceDrains
            | EngineKindName::EngineSource => {
                Ok(DiscardOutcome::Refused(AttemptRefusal::DiscardUnsupported))
            }
        }
    }
}
