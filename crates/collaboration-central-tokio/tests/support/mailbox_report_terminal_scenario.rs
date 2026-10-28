//! CS-1b containment: an invalid preserved report reaches a terminal SDK outcome.
#![deny(clippy::too_many_lines)]
#[path = "mailbox_report_task_host.rs"]
mod report_host;
#[path = "mailbox_report_task_store.rs"]
mod report_store;
use super::{
    mailbox_rule_fixture::{MailboxPair, ORIGINAL},
    relay_transport::{operation, relay_request},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::ConformanceResult;
use durable_steps::{Engine, OperationFailure, OperationState};
use fleet_sync_delivery::{
    DeliveryTaskConfig, LocalDeliveryConfig, ReportOutcome, delivery_request_key,
};
use fleet_sync_protocol::{DeliveryAnswer, DeliveryEnvelope, OperationAnswer, PeerId};
use report_host::{ReportHost, SharedPair};
use report_store::{ReportStore, limits, open_store, prepare_report};
use std::sync::Arc;
use tokio::sync::Mutex;

type ReportTask = ReportOutcome<ReportStore, sqlx_turso::Turso, BoardDeliveryTypes>;
const CANCEL: &str = "019f0000-0000-7000-8000-000000011085";

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
    let (item, cancel, batch, owner, relay) = {
        let mut pair = pair.lock().await;
        let item = pair.item(ORIGINAL, "literal original mailbox body")?;
        let accepted = pair.submit(&item).await?;
        if accepted.pointer("/value/value/value/kind") != Some(&serde_json::json!("accepted")) {
            return Err(format!("original admission failed: {accepted}").into());
        }
        // The cancel stays Local; it is deliberately never submitted to central.
        let cancel = pair.cancel(CANCEL)?;
        let fetched = operation(
            &mut pair.recipient.socket,
            relay_request(
                "fetch",
                serde_json::json!({
                    "stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000
                }),
            ),
        )
        .await?;
        let fetched: OperationAnswer<BoardConsumerProtocol> = serde_json::from_value(fetched)?;
        let OperationAnswer::Delivery(DeliveryAnswer::Fetch(batch)) = fetched else {
            return Err("expected real original Fetch".into());
        };
        let DeliveryEnvelope::Item(item) = item else {
            return Err("original must be an item".into());
        };
        let DeliveryEnvelope::Cancel(cancel) = cancel else {
            return Err("control must be a cancel".into());
        };
        (
            item,
            cancel,
            batch,
            PeerId::new(pair.recipient.peer)?,
            PeerId::new(pair.sender.peer)?,
        )
    };
    let directory = tempfile::tempdir()?;
    let host = Arc::new(ReportHost::new(pair));
    let store = open_store(&directory.path().join("recipient-report.db"), host.clone()).await?;
    let report = prepare_report(&store, &host, item, cancel, batch).await?;
    let mut engine = Engine::new(store, host.clone());
    engine.register(ReportTask::new(DeliveryTaskConfig {
        local: LocalDeliveryConfig {
            owner,
            limits: limits()?,
        },
        relay,
        fetch_items: 10,
        fetch_bytes: 1_000_000,
    }))?;
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
