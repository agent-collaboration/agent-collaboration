//! Recipient authority and report replay use the actual Fleet session transaction.
use super::{composed_fleet_fixture::*, fleet_mail_dispatch::*};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{AtomicStore, StoreError};
use fleet_mail_delivery::StatusStream;
use fleet_protocol::{
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
    // The report arrives as a kernel event, so its owed sends follow it.
    match collaboration_central_core::ObjectKernel::new(&fixture.runtime)
        .on_event(collaboration_central_core::ObjectEvent::Frame {
            binding: socket.binding,
            frame: serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
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
        })
        .await
    {
        // The event failed and its transaction rolled back. A shell then
        // closes that connection, as the Tokio and Cloudflare shells do: the
        // closed-socket event, and no further frames on it.
        collaboration_central_core::EventReply::SocketFailed => {
            collaboration_central_core::ObjectKernel::new(&fixture.runtime)
                .on_event(collaboration_central_core::ObjectEvent::SocketClosed {
                    binding: socket.binding,
                })
                .await;
            collaboration_central_core::ObjectPlatform::quarantine_connections(
                fixture.host.as_ref(),
                vec![socket.binding.connection],
            )
            .await;
            Err("the report's event failed and rolled back".into())
        }
        _ => Ok(frame),
    }
}

async fn owed_statuses(fixture: &Fixture) -> TestResult<i64> {
    use durable_steps::storage::TransactionScope;
    use sqlx_core::{query::query, row::Row};
    Ok(fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT count(*) AS owed FROM central_work_attempts WHERE kind='status' AND state='waiting'",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("owed")
                .map_err(StoreError::backend)
            })
        })
        .await?)
}

async fn sender_statuses(fixture: &Fixture) -> TestResult<Vec<DeliveryStatus<Acceptance>>> {
    Ok(fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                let owner = fleet_protocol::Submitter::Machine(
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
    // The submit's own event pushes to B; B takes custody.
    let frame = recipient.push(item.id()).await?;
    recipient
        .answer(
            &fixture,
            frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
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
            reason: fleet_protocol::DeliveryRefusal::NotRecipient
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
    // The failed event closed B's connection; B reconnects to report again.
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
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
async fn report_admission_owes_its_status_publication_without_a_wake() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    // The submit's own event pushes to B; B takes custody.
    let frame = recipient.push(item.id()).await?;
    recipient
        .answer(
            &fixture,
            frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
    fixture.io.advance(1_000);
    let before = fixture.io.wakes()?.len();
    let owed_before = owed_statuses(&fixture).await?;
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
    // The report owes the sender its new status publication in the same
    // transaction. The sender has no connection, so the row waits for its
    // Welcome: a waiting row is never due, so it asks for no near wake. The
    // only later wake is the relay record's 60-day mail retention.
    assert!(
        owed_statuses(&fixture).await? >= owed_before.max(1),
        "the report owed no status publication"
    );
    let horizon = fixture.io.now().unix_millis() + 86_400_000;
    let wakes = fixture.io.wakes()?;
    assert!(
        wakes[before..]
            .iter()
            .all(|wake| wake.unix_millis() > horizon),
        "a waiting status publication asked for a wake: {:?}",
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
        // The submit's own event pushes to B; B takes custody.
        let frame = recipient.push(item.id()).await?;
        recipient
            .answer(
                &fixture,
                frame,
                PushResult::Accepted {
                    accepted_at: fixture.io.now(),
                },
            )
            .await?;
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
    if report(&fixture, &mut recipient, id, 1, outcome).await?
        != (ReportResult::Ack {
            report_seq: ReportSeq::new(1),
        })
    {
        return Err("revoked recipient could not finish its addressed item".into());
    }
    if custody(&fixture, id).await? != ("delivered".into(), false) {
        return Err("allowed report did not advance status and release payload".into());
    }
    // The report's own event runs the revocation's pending session end, so
    // the revoked recipient's connection closes right after its answer.
    // Frames already queued before the close (its sender's notices) come first.
    let mut closed = None;
    for _ in 0..16 {
        match tokio::time::timeout(CAP, recipient.next()).await? {
            Ok(_) => {}
            Err(error) => {
                closed = Some(error.to_string());
                break;
            }
        }
    }
    match closed {
        Some(reason) if reason.contains("CredentialRejected(Revoked)") => {}
        other => return Err(format!("the revoked recipient was not closed: {other:?}").into()),
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
