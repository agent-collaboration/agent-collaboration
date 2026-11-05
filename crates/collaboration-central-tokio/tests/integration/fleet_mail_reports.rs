//! Recipient authority and report replay use the actual Fleet session transaction.
use super::{composed_fleet_fixture::*, fleet_mail_dispatch::*};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_delivery::StatusStream;
use fleet_sync_protocol::{
    ClientFrame, DeliveryAnswer, DeliveryRequest, DeliveryStatus, DeliveryTypes, FrameId, ItemId,
    OperationAnswer, OperationRequest, PushResult, RecipientOutcome, RecipientReport, ReportResult,
    ReportSeq, ServerFrame, StatusCursor,
};
use runtime_primitives::Clock;
use serde_json::json;

type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;

async fn report(
    fixture: &Fixture,
    socket: &mut Socket,
    item: ItemId,
    sequence: u64,
    outcome: RecipientOutcome<Acceptance>,
) -> TestResult<ReportResult> {
    let frame = send_report(fixture, socket, item, sequence, outcome).await?;
    loop {
        if let ServerFrame::Answer {
            frame: answered,
            body: OperationAnswer::Delivery(DeliveryAnswer::Report(answer)),
        } = socket.next().await?
        {
            assert_eq!(answered, frame);
            return Ok(answer);
        }
    }
}

async fn send_report(
    fixture: &Fixture,
    socket: &Socket,
    item: ItemId,
    sequence: u64,
    outcome: RecipientOutcome<Acceptance>,
) -> TestResult<FrameId> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let call = serde_json::from_value(
        json!({"operation":durable_steps::Uuid::now_v7(),"step":"recipient_report"}),
    )?;
    fixture
        .runtime
        .on_frame(
            socket.binding,
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Delivery(DeliveryRequest::Report {
                    call,
                    report: RecipientReport {
                        item,
                        report_seq: ReportSeq::new(sequence),
                        outcome,
                    },
                }),
            })?,
        )
        .await?;
    Ok(frame)
}

async fn sender_statuses(fixture: &Fixture) -> TestResult<Vec<DeliveryStatus<Acceptance>>> {
    Ok(fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                let owner = fleet_sync_protocol::Submitter::Machine(
                    A.to_owned().try_into().map_err(StoreError::backend)?,
                );
                let route = SERVICE_A
                    .to_owned()
                    .try_into()
                    .map_err(StoreError::backend)?;
                let page = StatusStream::<_, BoardDeliveryTypes>::new(scope, owner, route)
                    .fetch(owner, StatusCursor::new(0), 100)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(page.events.into_iter().map(|event| event.status).collect())
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The report contract independently compares recipient authority, replay and committed sender status."
)]
#[tokio::test]
async fn only_the_recipient_can_report_and_replay_never_advances_status_or_owes_it_twice()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let mut attacker = Socket::open(&fixture, C, SERVICE_C).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let frame = recipient
                .push(item.id())
                .await
                .map_err(StoreError::backend)?;
            recipient
                .answer(
                    &fixture,
                    frame,
                    PushResult::Accepted {
                        accepted_at: fixture.io.now(),
                    },
                )
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert_eq!(
        sender_statuses(&fixture).await?,
        vec![DeliveryStatus::AtRecipient]
    );

    assert_eq!(
        report(
            &fixture,
            &mut attacker,
            item.id(),
            1,
            RecipientOutcome::Held
        )
        .await?,
        ReportResult::Refused {
            reason: fleet_sync_protocol::DeliveryRefusal::NotRecipient
        }
    );
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(
        sender_statuses(&fixture).await?,
        vec![DeliveryStatus::AtRecipient]
    );

    fixture.sql("CREATE TRIGGER reject_report_status BEFORE INSERT ON _fleet_sync_status_owed WHEN NEW.status_kind='held' BEGIN SELECT RAISE(ABORT,'injected report status debt failure'); END").await?;
    assert!(
        send_report(&fixture, &recipient, item.id(), 1, RecipientOutcome::Held)
            .await
            .is_err()
    );
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(
        sender_statuses(&fixture).await?,
        vec![DeliveryStatus::AtRecipient],
        "status events must roll back with the report"
    );
    fixture.sql("DROP TRIGGER reject_report_status").await?;
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            1,
            RecipientOutcome::Held
        )
        .await?,
        ReportResult::Ack {
            report_seq: ReportSeq::new(1)
        }
    );
    let held = vec![DeliveryStatus::AtRecipient, DeliveryStatus::Held];
    assert_eq!(sender_statuses(&fixture).await?, held);
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            1,
            RecipientOutcome::Uncertain
        )
        .await?,
        ReportResult::Replayed {
            report_seq: ReportSeq::new(1)
        }
    );
    assert_eq!(sender_statuses(&fixture).await?, held);
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            2,
            RecipientOutcome::Uncertain
        )
        .await?,
        ReportResult::Ack {
            report_seq: ReportSeq::new(2)
        }
    );
    let terminal = vec![
        DeliveryStatus::AtRecipient,
        DeliveryStatus::Held,
        DeliveryStatus::Uncertain,
    ];
    assert_eq!(sender_statuses(&fixture).await?, terminal);
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            3,
            RecipientOutcome::Held
        )
        .await?,
        ReportResult::Ack {
            report_seq: ReportSeq::new(3)
        }
    );
    assert_eq!(
        sender_statuses(&fixture).await?,
        terminal,
        "a later sequence must not regress a final outcome"
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A report's new status job must contribute a committed wake instead of relying on an unrelated later alarm."
)]
#[tokio::test]
async fn report_admission_captures_its_post_commit_wake_in_the_session_adapter() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let frame = recipient
                .push(item.id())
                .await
                .map_err(StoreError::backend)?;
            recipient
                .answer(
                    &fixture,
                    frame,
                    PushResult::Accepted {
                        accepted_at: fixture.io.now(),
                    },
                )
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    // Drain only the setup publication. No alarm is driven after the Report.
    let setup = fixture.runtime.on_alarm().await?;
    assert!(setup.failed.is_empty(), "{setup:?}");
    fixture.io.advance(1_000);
    let now = fixture.io.now();
    let before = fixture.io.wakes()?.len();
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            1,
            RecipientOutcome::Held
        )
        .await?,
        ReportResult::Ack {
            report_seq: ReportSeq::new(1)
        }
    );
    let wakes = fixture.io.wakes()?;
    assert!(
        wakes.iter().skip(before).any(|wake| *wake == now),
        "new publication was admitted without a post-commit wake: {:?}",
        &wakes[before..]
    );
    assert_eq!(
        sender_statuses(&fixture).await?,
        vec![DeliveryStatus::AtRecipient, DeliveryStatus::Held]
    );
    fixture.close().await
}

async fn revoked_recipient_report(handoff: bool) -> TestResult {
    use collaboration_central_core::{AccessCaller, OwnerIdentity, RevocationDecision};
    use durable_steps::storage::TransactionScope;
    use sqlx_core::{query::query, row::Row};
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    if handoff {
        let (drive, ()) = tokio::try_join!(fixture.runtime.on_alarm(), async {
            let frame = recipient
                .push(item.id())
                .await
                .map_err(StoreError::backend)?;
            recipient
                .answer(
                    &fixture,
                    frame,
                    PushResult::Accepted {
                        accepted_at: fixture.io.now(),
                    },
                )
                .await
                .map_err(StoreError::backend)
        })?;
        if !drive.failed.is_empty() {
            return Err("handoff control failed".into());
        }
    }
    let id = item.id();
    let before = fixture.store.transaction(move |scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT handed_off_at IS NOT NULL AS handed_off FROM _fleet_sync_outbox_items WHERE item_id=?")
            .bind(id.as_uuid().to_string()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        row.try_get::<i64,_>("handed_off").map_err(StoreError::backend)
    })).await?;
    if (before == 1) != handoff {
        return Err("report fixture has the wrong handoff state".into());
    }
    let owner = OwnerIdentity::from_access(AccessCaller::Owner {
        email: "fixture@example.test".into(),
    })?;
    if fixture
        .runtime
        .revoke_machine(&owner, SERVICE_B.to_owned().try_into()?)
        .await?
        != RevocationDecision::MachineRevoked
    {
        return Err("recipient revoke was not admitted".into());
    }
    let revoked = fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT state FROM machine_enrollments WHERE machine_id=?",
                )
                .bind(B)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get::<String, _>("state")
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    if revoked != "revoked" {
        return Err("recipient row was not committed revoked".into());
    }
    let outcome = RecipientOutcome::Delivered {
        acceptance: serde_json::from_value(json!("queued"))?,
    };
    if report(&fixture, &mut recipient, id, 1, outcome.clone()).await?
        != (ReportResult::Ack {
            report_seq: ReportSeq::new(1),
        })
    {
        return Err("revoked recipient could not finish its addressed item".into());
    }
    if custody(&fixture, id).await? != ("delivered".into(), false) {
        return Err("allowed report did not advance status and release payload".into());
    }
    let events = sender_statuses(&fixture).await?;
    if report(&fixture, &mut recipient, id, 1, outcome).await?
        != (ReportResult::Replayed {
            report_seq: ReportSeq::new(1),
        })
        || sender_statuses(&fixture).await? != events
    {
        return Err("allowed report replay duplicated sender status".into());
    }
    fixture.close().await
}
#[tokio::test]
async fn revoked_live_recipient_can_report_an_addressed_item_after_handoff() -> TestResult {
    Box::pin(revoked_recipient_report(true)).await
}
#[tokio::test]
async fn revoked_live_recipient_can_report_an_addressed_item_without_a_handoff_witness()
-> TestResult {
    Box::pin(revoked_recipient_report(false)).await
}
