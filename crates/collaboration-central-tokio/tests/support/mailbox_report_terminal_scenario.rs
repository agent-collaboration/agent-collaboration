//! CS-1b containment: an invalid preserved report reaches a terminal SDK outcome.
#![deny(clippy::too_many_lines)]
#[path = "mailbox_report_task_host.rs"]
mod report_host;
#[path = "mailbox_report_task_store.rs"]
mod report_store;
use super::{
    mail_push_transport::{next_push, pushed},
    mailbox_rule_fixture::MailboxPair,
};
use collaboration_central_conformance::ConformanceResult;
use durable_steps::{Engine, OperationFailure, OperationState};
use fleet_mail_delivery::{DeliveryTaskConfig, LocalDeliveryConfig, delivery_request_key};
use fleet_protocol::{CancelBody, DeliveryCancel, DeliveryEnvelope, ItemId, PeerId, ServerCall};
use report_host::{ReportHost, SharedPair};
use report_store::{ReportSenderBinding, ReportTask, limits, open_store, prepare_report};
use std::sync::Arc;
use tokio::sync::Mutex;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let pair = Arc::new(Mutex::new(MailboxPair::open(base).await?));
    let proof = run_report(pair.clone()).await;
    let pair = Arc::try_unwrap(pair)
        .map_err(|_| "report probe retained its socket owner")?
        .into_inner();
    let closed = pair.close().await;
    match (proof, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(proof), Err(cleanup)) => {
            Err(format!("report containment: {proof}; cleanup: {cleanup}").into())
        }
    }
}
async fn run_report(pair: SharedPair) -> ConformanceResult<()> {
    let (item, cancel, submitter, frame, owner, relay, now) = {
        let mut pair = pair.lock().await;
        let identity = durable_steps::Uuid::now_v7();
        let now = item_creation_time(identity)?;
        let item = pair.item(&identity.to_string(), "literal original mailbox body")?;
        let accepted = pair.submit(&item).await?;
        if accepted.pointer("/value/value/value/kind") != Some(&serde_json::json!("accepted")) {
            return Err(format!("original admission failed: {accepted}").into());
        }
        // The cancel stays Local; it is deliberately never submitted to central.
        let cancel = DeliveryCancel::new(
            ItemId::new(durable_steps::Uuid::now_v7())?,
            item.sender().clone(),
            item.recipient().clone(),
            None,
            CancelBody { target: item.id() },
        )?;
        let (
            frame,
            ServerCall::Push {
                submitter,
                item: received,
                ..
            },
        ) = next_push(&mut pair.recipient.socket).await?
        else {
            return Err("expected real original Push".into());
        };
        if serde_json::to_vec(&received)? != serde_json::to_vec(&item)? {
            return Err("Push changed the original sealed bytes".into());
        }
        let DeliveryEnvelope::Item(item) = item else {
            return Err("original must be an item".into());
        };
        (
            item,
            cancel,
            submitter,
            frame,
            PeerId::new(pair.recipient.peer)?,
            PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
            now,
        )
    };
    let directory = tempfile::tempdir()?;
    let host = Arc::new(ReportHost::new(pair.clone(), now));
    let store = open_store(&directory.path().join("recipient-report.db"), host.clone()).await?;
    let (report, receipt) = prepare_report(&store, &host, item, cancel, submitter, relay).await?;
    pushed(&mut pair.lock().await.recipient.socket, frame, receipt).await?;
    let mut engine = Engine::new(store, host.clone());
    engine.register(fleet_mail_delivery::RemoveDeliveryRecord::<
        report_store::ReportStore,
        sqlx_turso::Turso,
    >::new(fleet_mail_delivery::DeliveryHostRole::Machine))?;
    engine.register(ReportTask::new(
        DeliveryTaskConfig {
            local: LocalDeliveryConfig {
                owner,
                limits: limits()?,
            },
            relay,
        },
        host.clone(),
        Arc::new(ReportSenderBinding),
    ))?;
    let status = engine
        .start_and_settle::<ReportTask>(
            &delivery_request_key(&(report.item, report.report_seq))?,
            report,
        )
        .await?;
    // Drive beyond every maximum backoff. A failed operation must never dispatch again.
    for millis in [60_001, 3_600_000, 86_400_000] {
        host.advance(millis);
        engine.drive_due().await?;
        if host.dispatch_attempts() != 1 {
            return Err(format!(
                "report task retried dispatch: expected1 attempt; observed{}",
                host.dispatch_attempts()
            )
            .into());
        }
    }
    if !host.terminal_refusal() {
        return Err("real central terminal processing refusal was not received".into());
    }
    if !matches!(status.state, OperationState::Failed { reason: OperationFailure::Refused { ref code, .. } } if code == "executionFailed")
    {
        return Err(format!("refused report did not terminate: {:?}", status.state).into());
    }
    println!(
        "MAILBOX_REPORT_TERMINAL_CASE preserved_local_prevention_refused; report_dispatch_attempts=1; future_due_scans=3; terminal=failed"
    );
    Ok(())
}

fn item_creation_time(
    identity: durable_steps::Uuid,
) -> ConformanceResult<runtime_primitives::SyncTime> {
    let seconds = identity
        .get_timestamp()
        .ok_or("UUIDv7 timestamp missing")?
        .to_unix()
        .0;
    Ok(runtime_primitives::SyncTime::from_unix_millis(
        i64::try_from(seconds)? * 1_000,
    ))
}
