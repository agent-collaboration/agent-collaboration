//! Core's closed admin kind registry and discard routing.
use crate::{WorkspaceRuntime, organization_admin_contract::EngineKindName};
use central_storage::StoreFailure;
use fleet_engine::{AttemptRefusal, DiscardOutcome, KindRegistry, RegisteredDiscard};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_schedules::automation_finish_store;
use fleet_timer_policy::FinishEvent;
use fleet_wakes::wake_finish_store;

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

pub(crate) async fn finish_registered_discard<H: CentralStore, S: WorkspaceStores>(
    registered: RegisteredDiscard<'_, WorkspaceRuntime<H, S>>,
) -> Result<DiscardOutcome, StoreFailure> {
    match registered.kind() {
        EngineKindName::WakeTimer => {
            registered
                .finish::<fleet_wakes::WakeFinish>(wake_finish_store(), FinishEvent::Discard)
                .await
        }
        EngineKindName::AutomationTimer => {
            registered
                .finish::<fleet_schedules::AutomationFinish>(
                    automation_finish_store(),
                    FinishEvent::Discard,
                )
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
