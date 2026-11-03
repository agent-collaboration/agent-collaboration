//! Already-admitted Fleet custody waits for its revoked recipient's readmission.
use super::{
    composed_fleet_fixture::*,
    fleet_mail_dispatch::{CAP, Socket, custody, envelope, submit, submit_answer},
};
use collaboration_central_core::{AccessCaller, OwnerIdentity, RevocationDecision};
use collaboration_central_tokio::SocketCommand;
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_protocol::{DeliveryStatus, ItemId, ItemStatus, ItemStatusState};
use serde_json::json;

type Acceptance =
    <board_sync_protocol::BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance;

async fn status(fixture: &Fixture, item: ItemId) -> TestResult<ItemStatus<Acceptance>> {
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_sync_delivery::Outbox::<_, board_sync_protocol::BoardDeliveryTypes>::new(
                    scope,
                    collaboration_central_core::central_mailbox_limits()
                        .map_err(StoreError::backend)?,
                )
                .relay_query(A.to_owned().try_into().map_err(StoreError::backend)?, item)
                .await
                .map_err(fleet_sync_delivery::delivery_rollback)
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A real mpsc send selected before revoke may finish, but cannot duplicate, advance custody without a receipt, or select again after the committed gate."
)]
#[tokio::test]
async fn selected_push_held_by_native_backpressure_can_finish_after_revoke_without_another_selection()
-> TestResult {
    use board_sync_protocol::BoardConsumerProtocol;
    use durable_steps::storage::TransactionScope;
    use fleet_sync_protocol::{ServerCall, ServerFrame, ServerRequest};
    use sqlx_core::{query::query, row::Row};
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    recipient.fill_send_queue(&fixture).await?;
    let queued = recipient.queued_commands();
    assert!(queued > 0);
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let mut selected = fixture.io.reply_deadlines.subscribe();
    let before = *selected.borrow();
    let proof = Box::pin(tokio::time::timeout(CAP, async {
        tokio::try_join!(
            async {
                fixture
                    .runtime
                    .on_alarm()
                    .await
                    .map_err(Box::<dyn std::error::Error + Send + Sync>::from)
            },
            async {
                selected.wait_for(|count| *count == before + 1).await?;
                // The deadline future is polled only after begin_peer_request has
                // selected a binding and the real channel send is blocked.
                assert_eq!(
                    fixture
                        .runtime
                        .revoke_machine(&owner()?, SERVICE_B.to_owned().try_into()?)
                        .await?,
                    RevocationDecision::MachineRevoked
                );
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
                assert_eq!(revoked, "revoked");
                let ended = async {
                    fixture
                        .runtime
                        .on_alarm()
                        .await
                        .map_err(Box::<dyn std::error::Error + Send + Sync>::from)
                };
                let release_channel = async {
                    for _ in 0..queued {
                        assert!(matches!(
                            recipient.next().await?,
                            ServerFrame::Welcome { .. }
                        ));
                    }
                    let mut pushes = 0;
                    loop {
                        match recipient.next_command().await? {
                            SocketCommand::Frame(bytes) => match serde_json::from_slice::<
                                ServerFrame<BoardConsumerProtocol>,
                            >(
                                &bytes
                            )? {
                                ServerFrame::Request {
                                    body:
                                        ServerRequest::Crate(ServerCall::Push { item: pushed, .. }),
                                    ..
                                } => {
                                    pushes += 1;
                                    assert_eq!(pushed.id(), item.id());
                                    assert_eq!(pushed.digest(), item.digest());
                                    // No Inbox receipt is manufactured from a revoked peer.
                                }
                                ServerFrame::Closed { reason } => assert_eq!(
                                    serde_json::to_value(reason)?,
                                    json!({"kind":"credentialRejected","value":"revoked"})
                                ),
                                other => {
                                    return Err(format!(
                                        "unexpected queued frame at revoke edge: {}",
                                        serde_json::to_string(&other)?
                                    )
                                    .into());
                                }
                            },
                            SocketCommand::Close(reason) => {
                                assert_eq!(
                                    serde_json::to_value(reason)?,
                                    json!({"kind":"credentialRejected","value":"revoked"})
                                );
                                break;
                            }
                        }
                    }
                    assert!(pushes <= 1, "a pre-commit selection duplicated its Push");
                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                };
                let (end_report, ()) = tokio::try_join!(ended, release_channel)?;
                assert!(end_report.failed.is_empty(), "{end_report:?}");
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            }
        )
    }))
    .await;
    let proof = async {
        let (drive, ()) = proof??;
        assert!(drive.failed.is_empty(),"{drive:?}");
        assert_eq!(custody(&fixture,item.id()).await?,("sent".into(),true));
        assert!(matches!(status(&fixture,item.id()).await?.state,ItemStatusState::Known {status:DeliveryStatus::Sent,version,..} if version.value()==1));
        assert_eq!(*selected.borrow(),before+1);
        fixture.io.advance(10_000);
        assert!(fixture.runtime.on_alarm().await?.failed.is_empty());
        assert_eq!(*selected.borrow(),before+1,"revoked peer acquired a new selected send");
        assert_eq!(custody(&fixture,item.id()).await?,("sent".into(),true));
        assert!(matches!(status(&fixture,item.id()).await?.state,ItemStatusState::Known {status:DeliveryStatus::Sent,version,..} if version.value()==1));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.close().await?;
    proof
}

fn owner() -> TestResult<OwnerIdentity> {
    Ok(OwnerIdentity::from_access(AccessCaller::Owner {
        email: "fixture@example.test".into(),
    })?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Revocation must close the actual recipient socket without sending queued custody or changing Sent/content."
)]
#[tokio::test]
async fn revoked_recipient_gets_no_queued_push_and_fleet_keeps_sent_content() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let before = custody(&fixture, item.id()).await?;
    assert_eq!(before, ("sent".into(), true));
    let proof = async {
        let revoke = async {
            assert_eq!(
                fixture
                    .runtime
                    .revoke_machine(&owner()?, SERVICE_B.to_owned().try_into()?)
                    .await?,
                RevocationDecision::MachineRevoked
            );
            // The sender admits the receiving EndSessions task in this Fleet
            // journal. This fixture explicitly drives that newly committed wake.
            let ended = fixture.runtime.on_alarm().await?;
            assert!(ended.failed.is_empty(), "{ended:?}");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let no_push = async {
            match recipient.next_command().await? {
                SocketCommand::Frame(bytes) => assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes)?,
                    json!({"kind":"closed","value":{"reason":{"kind":"credentialRejected","value":"revoked"}}}),
                    "revocation sent a queued frame instead of its protocol Closed"),
                SocketCommand::Close(_) => return Err("revocation omitted its protocol Closed frame".into()),
            }
            match recipient.next_command().await? {
                SocketCommand::Close(reason) => assert_eq!(serde_json::to_value(reason)?, json!({"kind":"credentialRejected","value":"revoked"})),
                SocketCommand::Frame(_) => return Err("revocation sent another frame before physical close".into()),
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        Box::pin(tokio::time::timeout(CAP, async {
            tokio::try_join!(revoke, no_push)
        }))
        .await??;
        assert_eq!(custody(&fixture, item.id()).await?, before);
        fixture.io.advance(10_000);
        let driven = fixture.runtime.on_alarm().await?;
        assert!(driven.failed.is_empty(), "{driven:?}");
        assert_eq!(custody(&fixture, item.id()).await?, before);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.close().await?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The public SDK must retain Sent until its creation deadline, remove it exactly at the deadline, and refuse an older immutable id."
)]
#[tokio::test]
async fn never_readmitted_recipient_custody_is_removed_at_day_sixty_and_then_too_old() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    assert_eq!(
        fixture
            .runtime
            .revoke_machine(&owner()?, SERVICE_B.to_owned().try_into()?)
            .await?,
        RevocationDecision::MachineRevoked
    );
    fixture.runtime.on_alarm().await?;
    let deadline = fleet_sync_delivery::removal_deadline(item.id()).unix_millis();
    fixture.io.advance(deadline - NOW - 1);
    assert!(fixture.runtime.on_alarm().await?.failed.is_empty());
    assert!(matches!(
        status(&fixture, item.id()).await?.state,
        ItemStatusState::Known {
            status: DeliveryStatus::Sent,
            ..
        }
    ));
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    fixture.io.advance(1);
    assert!(fixture.runtime.on_alarm().await?.failed.is_empty());
    assert!(matches!(
        status(&fixture, item.id()).await?.state,
        ItemStatusState::Unknown
    ));
    fixture.io.advance(1);
    let answer = submit_answer(&fixture, &item).await?;
    assert_eq!(
        answer.pointer("/value/value/kind"),
        Some(&json!("refused")),
        "{answer}"
    );
    assert_eq!(
        answer.pointer("/value/value/value/reason/kind"),
        Some(&json!("tooOld")),
        "{answer}"
    );
    assert!(matches!(
        status(&fixture, item.id()).await?.state,
        ItemStatusState::Unknown
    ));
    fixture.close().await
}
