//! The real service and SDK retain a supplied S2 fault; central fault persistence is a separate gate.
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, FaultedAutomationSnapshot, ScheduleShowRequest,
};
use collaboration_runtime::{CentralAutomationError, CentralAutomationTransport};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[path = "support/served_api.rs"]
mod served_api;

struct FaultedCatalog {
    fault: FaultedAutomationSnapshot,
    reads: AtomicUsize,
}
impl CentralAutomationTransport for FaultedCatalog {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            // Count every call so an unexpected call still fails the exact-count assertions.
            self.reads.fetch_add(1, Ordering::SeqCst);
            match request {
                AutomationCall::ScheduleShow(request)
                    if request.schedule_id.as_str() == self.fault.automation_id.as_str() =>
                {
                    Ok(AutomationAnswer::Faulted(self.fault.clone()))
                }
                AutomationCall::List(_) => Ok(AutomationAnswer::List {
                    automations: vec![collaboration_protocol::CatalogAutomation::Faulted(
                        self.fault.clone(),
                    )],
                    next: Some("actual-central-continuation".into()),
                }),
                _ => Err(CentralAutomationError::InvalidWork),
            }
        })
    }
}

fn schedule_fault(
    id: &collaboration_protocol::ScheduleId,
) -> Result<FaultedAutomationSnapshot, Box<dyn std::error::Error>> {
    Ok(FaultedAutomationSnapshot {
        automation_id: id.as_str().to_owned().try_into()?,
        kind: collaboration_protocol::AutomationKind::Schedule,
        revision: collaboration_protocol::ChangeId::generate(),
        fault: collaboration_protocol::AutomationFault {
            error: collaboration_protocol::AutomationRefusal::InvalidRecord {
                field: "definition_json".into(),
            },
            faulted_at: "2026-11-01T12:00:00Z".to_owned().try_into()?,
        },
    })
}

#[tokio::test]
async fn export_reports_the_complete_fault_before_reading_instruction_or_run_continuity()
-> Result<(), Box<dyn std::error::Error>> {
    let id = collaboration_protocol::ScheduleId::generate();
    let fault = schedule_fault(&id)?;
    let transport = Arc::new(FaultedCatalog {
        fault: fault.clone(),
        reads: AtomicUsize::new(0),
    });
    // This stand-in is the existing S2 transport seam; it does not prove a persisted central fault.
    let selection_root = tempfile::tempdir()?;
    let selection_store = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&selection_root.path().join("automation.sqlite"))
            .await?,
    ));
    let served = served_api::ServedApi::start(
        collaboration_service::ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_catalog(Some(transport.clone()))
        .with_automation_store(selection_store.clone()),
    )?;
    let proof: Result<(), Box<dyn std::error::Error>> = async {
        let client = served.client("faulted-export").await?;
        let result = client
            .export_schedule(ScheduleShowRequest { schedule_id: id })
            .await;
        let Err(collaboration_client::ScheduleClientError::ReadFailure(error)) = result else {
            return Err(
                "service replaced a valid faulted Schedule with a package or generic failure"
                    .into(),
            );
        };
        if serde_json::to_value(error.fault())? != serde_json::to_value(fault)?
            || transport.reads.load(Ordering::SeqCst) != 1
        {
            return Err(
                "faulted export lost shared fault/revision evidence or repeated its read".into(),
            );
        }
        Ok(())
    }
    .await;
    let stopped = served.stop().await;
    Arc::try_unwrap(selection_store)
        .map_err(|_| "fault fixture ownership storage still in use")?
        .into_inner()
        .close()
        .await?;
    selection_root.close()?;
    proof?;
    stopped?;
    Ok(())
}

#[tokio::test]
async fn schedule_show_and_list_preserve_the_common_fault_envelope_and_actual_cursor()
-> Result<(), Box<dyn std::error::Error>> {
    let id = collaboration_protocol::ScheduleId::generate();
    let fault = schedule_fault(&id)?;
    let transport = Arc::new(FaultedCatalog {
        fault: fault.clone(),
        reads: AtomicUsize::new(0),
    });
    let selection_root = tempfile::tempdir()?;
    let selection_store = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&selection_root.path().join("automation.sqlite"))
            .await?,
    ));
    let served = served_api::ServedApi::start(
        collaboration_service::ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_catalog(Some(transport.clone()))
        .with_automation_store(selection_store.clone()),
    )?;
    let proof: Result<(), Box<dyn std::error::Error>> = async {
        let client = served.client("faulted-schedule-reads").await?;
        let shown = serde_json::to_value(
            client
                .read_schedule(ScheduleShowRequest { schedule_id: id })
                .await?,
        )?;
        let page = client
            .list_schedules(collaboration_protocol::AutomationPageRequest {
                cursor: None,
                limit: 1_u32.try_into()?,
            })
            .await?;
        let listed = serde_json::to_value(&page.records)?;
        for response in [&shown, &listed[0]] {
            if response["kind"] != "faulted"
                || response["fault"] != serde_json::to_value(&fault)?
                || !response["message"].is_string()
                || response.get("value").is_some()
            {
                return Err(
                    "Schedule read changed the shared fault or omitted its common envelope".into(),
                );
            }
        }
        use base64::Engine as _;
        let cursor = page
            .next_cursor
            .as_deref()
            .ok_or("faulted central continuation missing")?;
        let cursor: serde_json::Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(cursor)?,
        )?;
        if cursor["collection"] != "schedule"
            || cursor["position"]["kind"] != "central"
            || cursor["position"]["after"] != "actual-central-continuation"
            || transport.reads.load(Ordering::SeqCst) != 2
        {
            return Err("faulted list invented a cursor or repeated a read".into());
        }
        Ok(())
    }
    .await;
    let stopped = served.stop().await;
    Arc::try_unwrap(selection_store)
        .map_err(|_| "fault fixture ownership storage still in use")?
        .into_inner()
        .close()
        .await?;
    selection_root.close()?;
    proof?;
    stopped?;
    Ok(())
}
