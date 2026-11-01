//! Real authenticated Session→Fleet custody and receiving SQLite; no model or native turn.
use super::*;
use automation_storage::{AutomationFaultPoint, AutomationStore, CentralLocalWork};
use board_central_test_support::{
    ObjectAddress, RuntimeControl, RuntimeControlRequest, TestCentral,
};
use collaboration_central_conformance::{AutomationClockControl, ConformanceClient};
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationCall, FiringOutcome, WorkEntry,
};
use collaboration_runtime::{
    AutomationFaultSelector, CentralAutomationClock, CentralAutomationTransport,
    DebugAutomationFaultController, SessionAutomationTransport,
};
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::Mutex;
#[path = "central_automation_fixture.rs"]
mod fixture;
use fixture::*;
#[path = "central_automation_reply_loss.rs"]
mod reply_loss;

#[tokio::test]
async fn central_automation_hint_admits_before_ack_and_reports_to_real_central() -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let now = chrono::DateTime::parse_from_rfc3339(DUE)?.timestamp_millis();
    let central = start_central(&directory.path().join("central")).await?;
    let faults = Arc::new(DebugAutomationFaultController::default());
    let arm = faults
        .arm(
            AutomationFaultPoint::BatchAdmissionCommittedBeforeAck,
            AutomationFaultSelector::default(),
        )
        .await?;
    let clock = Arc::new(ControlledClock::new(now));
    let host = ReceiverHost::open(&root, &central, clock.clone(), Some(faults.clone())).await?;
    let proof: ProofResult = async {
        require(
            pending(&host.transport).await?.is_empty(),
            "fresh machine has custody",
        )?;
        create_schedule(
            &host.transport,
            collaboration_protocol::TimingRequest::Interval {
                seconds: 600.try_into()?,
                first_due: None,
            },
            "Actual central frozen instruction",
        )
        .await?;
        advance(&central, DUE).await?;
        let held = timeout(BOUND, faults.wait_held(&arm)).await??;
        let batch_id = held.evidence.batch_id.ok_or("held batch missing")?;
        let retained = pending(&host.transport).await?;
        require(
            retained.len() == 1 && retained[0].batch_id == batch_id,
            "central lost custody before ACK",
        )?;
        let batch: AutomationBatch = serde_json::from_slice(&retained[0].frozen_bytes)?;
        let Some(WorkEntry::Firing(firing)) = batch.entries.entries().first() else {
            return Err("Firing missing".into());
        };
        let receipt = host
            .store
            .lock()
            .await
            .read_central_firing(&firing.firing_id)
            .await?
            .ok_or("durable receipt absent before ACK")?;
        require(
            matches!(receipt.local_work, CentralLocalWork::Schedule { .. }),
            "local run absent",
        )?;
        require(
            host.reports.flush_due().await? == Some(now + 500),
            "sparse deadline moved",
        )?;
        faults.release(&arm).await?;
        host.receiver.receive_pending().await?;
        require(
            pending(&host.transport).await?.is_empty(),
            "ACK did not retire real custody",
        )?;
        let before = show(&host.transport, &firing.firing_id).await?;
        require(
            before.report_seq.is_none(),
            "admission fabricated a machine report",
        )?;
        require(
            matches!(before.outcome, FiringOutcome::Received),
            "ACK did not mark receipt",
        )?;
        clock.advance(now + 500);
        require(
            host.reports.flush_due().await?.is_none(),
            "accepted reports remain owed",
        )?;
        let reported = show(&host.transport, &firing.firing_id).await?;
        require(
            reported.report_seq.map(u64::from) == Some(1),
            "report sequence lost",
        )?;
        require(
            serde_json::to_value(&reported.outcome)? == serde_json::to_value(&receipt.outcome)?,
            "report outcome lost",
        )?;
        Ok(())
    }
    .await;
    let stopped = host.close().await;
    let central_stopped = central.shutdown().await;
    proof?;
    stopped?;
    central_stopped?;
    Ok(())
}

#[tokio::test]
async fn central_automation_reconnect_recovers_expired_notice_and_large_referenced_content()
-> TestResult {
    use super::super::machine_notice_reactions::MachineNoticeReactions;
    use board_sync_protocol::{
        BoardConsumerAnswer, BoardConsumerRequest, BoardReadCursor, BoardReadPage, BoardReadScope,
        NoticeRead,
    };
    use fleet_sync_protocol::{NoticePosition, OperationAnswer, OperationRequest};
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let now = chrono::DateTime::parse_from_rfc3339(DUE)?.timestamp_millis();
    let central = start_central(&directory.path().join("central")).await?;
    let clock = Arc::new(ControlledClock::new(now));
    let host = ReceiverHost::open(&root, &central, clock.clone(), None).await?;
    let text = "x".repeat(1024 * 1024);
    let prepared: ProofResult = async {
        host.receiver.receive_pending().await?;
        create_schedule(
            &host.transport,
            collaboration_protocol::TimingRequest::At {
                at: DUE.to_owned().try_into()?,
            },
            &text,
        )
        .await?;
        Ok(())
    }
    .await;
    let closed = host.close().await;
    if let Err(error) = prepared.and(closed) {
        central.shutdown().await?;
        return Err(error);
    }
    // The first callback freezes custody while the target is absent. Aging keeps
    // those exact Batch bytes but prunes their seven-day notice.
    advance(&central, DUE).await?;
    let aged = "2026-11-08T10:05:00Z";
    advance(&central, aged).await?;
    clock.advance(chrono::DateTime::parse_from_rfc3339(aged)?.timestamp_millis());
    let faults = Arc::new(DebugAutomationFaultController::default());
    let arm = faults
        .arm(
            AutomationFaultPoint::BatchAdmissionCommittedBeforeAck,
            AutomationFaultSelector::default(),
        )
        .await?;
    let host = ReceiverHost::open(&root, &central, clock.clone(), Some(faults.clone())).await?;
    let proof: ProofResult = async {
        let held = timeout(BOUND, faults.wait_held(&arm)).await??;
        let retained = pending(&host.transport).await?;
        require(
            retained.len() == 1,
            "offline custody was lost or duplicated",
        )?;
        require(
            held.evidence.batch_id.as_ref() == Some(&retained[0].batch_id),
            "reconnect admitted another Batch",
        )?;
        require(
            retained[0].frozen_bytes.len() < 16 * 1024,
            "instruction was inlined into the Batch",
        )?;
        let batch: AutomationBatch = serde_json::from_slice(&retained[0].frozen_bytes)?;
        let Some(WorkEntry::Firing(firing)) = batch.entries.entries().first() else {
            return Err("offline Firing missing".into());
        };
        let receipt = host
            .store
            .lock()
            .await
            .read_central_firing(&firing.firing_id)
            .await?
            .ok_or("offline local receipt absent")?;
        let CentralLocalWork::Schedule { .. } = receipt.local_work else {
            return Err("offline run missing".into());
        };
        let collaboration_protocol::FiringWork::Schedule { definition } = &firing.work else {
            return Err("offline work changed kind".into());
        };
        let local_content = host
            .store
            .lock()
            .await
            .read_instruction(&definition.instruction_id)
            .await?;
        require(
            local_content.text.as_str() == text,
            "referenced instruction bytes changed",
        )?;
        let requests = host.projects.requests.clone();
        let notices = requests
            .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
                scope: BoardReadScope::Notices,
                cursor: Some(BoardReadCursor::Notices(NoticePosition::new(0))),
            }))
            .await?;
        let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(
            NoticeRead::CursorExpired { floor },
        ))) = notices
        else {
            return Err("real eight-day notice cursor did not expire".into());
        };
        require(
            floor > NoticePosition::new(0),
            "notice expiry did not advance floor",
        )?;
        faults.release(&arm).await?;
        let progress_root = directory.path().join("component-notice-progress");
        let mut reactions = MachineNoticeReactions::open(&progress_root).await?;
        reactions.attach_automations(host.receiver.clone());
        reactions
            .catch_up_notices(
                &host.projects,
                &Arc::downgrade(&host.admission),
                &host.fleet,
                &Weak::new(),
                &requests,
            )
            .await?;
        let progress = board_fleet_sync::MachineNoticeProgressStore::open(&progress_root)
            .await?
            .load()
            .await?;
        require(
            progress >= floor,
            "expired cursor saved before recovering owners",
        )?;
        host.receiver.receive_pending().await?;
        require(
            pending(&host.transport).await?.is_empty(),
            "offline ACK did not retire custody",
        )?;
        use sqlx::Connection;
        let mut sql = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(root.join("automation.sqlite")),
        )
        .await?;
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM workflow_runs),(SELECT count(*) FROM firing_receipts)",
        )
        .fetch_one(&mut sql)
        .await?;
        sql.close().await?;
        require(
            counts == (1, 1),
            "reconnect duplicated or dropped local work",
        )?;
        clock.advance(clock.now_ms() + 500);
        require(
            host.reports.flush_due().await?.is_none(),
            "offline reports remain owed",
        )?;
        require(
            show(&host.transport, &firing.firing_id)
                .await?
                .report_seq
                .map(u64::from)
                == Some(1),
            "offline report sequence lost",
        )?;
        Ok(())
    }
    .await;
    let stopped = host.close().await;
    let central_stopped = central.shutdown().await;
    proof?;
    stopped?;
    central_stopped?;
    Ok(())
}
