//! Reopen the actual central owner after Host shutdown; no second catalog implementation.
use super::super::runtime_fixture::CENTRAL_NOW;
use super::{ExpectedResources, ProofResult, RuntimePrefix, StoreExpectation, ensure};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationId, FiringListRequest, ScheduleShowRequest,
    WakeShowRequest, WakeState,
};
use collaboration_runtime::{CentralAutomationTransport, SessionAutomationTransport};
use serde_json::json;
use std::path::Path;

pub(super) async fn check_reopened_catalog(
    directory: &Path,
    expected: &ExpectedResources,
    expectation: StoreExpectation,
) -> ProofResult<()> {
    if expected.wake.is_none() && expected.schedule.is_none() {
        return Ok(());
    }
    let service = collaboration_runtime::load_service_identity(directory)?;
    let identity = collaboration_contracts::MachineIdentity::new(service.clone(), None)?;
    let origin = message_board::ActorOrigin {
        installation: String::from(service).try_into()?,
        machine: identity.machine_label().as_str().to_owned().try_into()?,
    };
    let central=board_central_test_support::TestCentral::start_with_peer_and_controls(
        &directory.join("test-central"),&origin,collaboration_runtime::load_installation_peer(directory)?,
        vec![serde_json::from_value(json!({"address":{"kind":"fleet"},"command":{"kind":"automationClock","value":{"kind":"start","now":CENTRAL_NOW}}}))?],
    ).await?;
    let session_root = tempfile::Builder::new()
        .prefix("catalog-readback-")
        .tempdir_in(directory)?;
    let (requests, runtime) = central.connect_machine_session(session_root.path()).await?;
    let transport = SessionAutomationTransport::new(requests);
    let proof: ProofResult<()> = async {
        let retired = matches!(expectation, StoreExpectation::Retired);
        if let Some((_, id)) = &expected.wake {
            let AutomationAnswer::Wake(wake) = transport
                .call(AutomationCall::WakeShow(WakeShowRequest {
                    wakeup_id: id.clone(),
                }))
                .await?
            else {
                return Err("reopened central Wake missing".into());
            };
            ensure(
                wake.definition.wakeup_id == *id
                    && wake.state
                        == if retired {
                            WakeState::Cancelled
                        } else {
                            WakeState::Active
                        }
                    && wake.next_due_at.is_none() == retired
                    && wake.first_firing.is_none(),
                "reopened central Wake did not preserve literal retirement and no-firing state",
            )?;
            empty_firings(&transport, id.as_str().to_owned().try_into()?).await?;
        }
        if let Some((_, id)) = &expected.schedule {
            let AutomationAnswer::Schedule(schedule) = transport
                .call(AutomationCall::ScheduleShow(ScheduleShowRequest {
                    schedule_id: id.clone(),
                }))
                .await?
            else {
                return Err("reopened central Schedule missing".into());
            };
            let enabled = matches!(
                expectation,
                StoreExpectation::Unchanged(RuntimePrefix::EnabledSchedule)
            );
            ensure(
                schedule.schedule_id == *id
                    && schedule.definition.enabled == enabled
                    && schedule.next_due_at.is_some() == enabled,
                "reopened central Schedule did not preserve literal enabled state",
            )?;
            empty_firings(&transport, id.as_str().to_owned().try_into()?).await?;
        }
        Ok(())
    }
    .await;
    drop(transport);
    let stopped = runtime.shutdown().await;
    let removed = session_root.close();
    proof?;
    stopped?;
    removed?;
    Ok(())
}
async fn empty_firings(
    transport: &SessionAutomationTransport,
    id: AutomationId,
) -> ProofResult<()> {
    let AutomationAnswer::Firings(page) = transport
        .call(AutomationCall::FiringList(FiringListRequest {
            automation_id: id,
            cursor: None,
            limit: 100.try_into()?,
        }))
        .await?
    else {
        return Err("reopened central Firing list missing".into());
    };
    ensure(
        page.records.is_empty() && page.next_cursor.is_none(),
        "cleanup fixture admitted unexpected central work",
    )
}
