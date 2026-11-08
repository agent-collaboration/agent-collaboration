//! Already-admitted Fleet custody waits for its revoked recipient's readmission.
use super::{
    composed_fleet_fixture::*,
    fleet_mail_dispatch::{CAP, Socket, custody, envelope, submit, submit_answer},
};
use collaboration_central_core::{AccessCaller, OwnerIdentity, RevocationDecision};
use collaboration_central_tokio::SocketCommand;
use durable_steps::{AtomicStore, StoreError};
use fleet_protocol::{DeliveryStatus, ItemId, ItemStatus, ItemStatusState};
use serde_json::json;

type Acceptance =
    <board_sync_protocol::BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance;

async fn status(fixture: &Fixture, item: ItemId) -> TestResult<ItemStatus<Acceptance>> {
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_mail_delivery::Outbox::<_, board_sync_protocol::BoardDeliveryTypes, _>::new(
                    scope,
                    collaboration_central_core::fleet_outbox_limits()
                        .map_err(StoreError::backend)?,
                    &collaboration_central_core::FleetSenderBinding,
                )
                .relay_query(
                    fleet_protocol::Submitter::Machine(
                        A.to_owned().try_into().map_err(StoreError::backend)?,
                    ),
                    item,
                )
                .await
                .map_err(fleet_mail_delivery::delivery_rollback)
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A revoked recipient cannot admit new custody; its earlier queued item and task must remain unchanged."
)]
#[tokio::test]
async fn new_submit_to_revoked_recipient_is_refused_without_changing_existing_custody() -> TestResult
{
    use super::fleet_mail_admission::admitted_rows;
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let earlier = envelope(B, SERVICE_B)?;
    submit(&fixture, &earlier).await?;
    assert_eq!(
        fixture
            .runtime
            .revoke_machine(&owner()?, SERVICE_B.to_owned().try_into()?)
            .await?,
        RevocationDecision::MachineRevoked
    );
    let before = admitted_rows(&fixture).await?;
    let replay = submit_answer(&fixture, &earlier).await?;
    assert_eq!(
        replay.pointer("/value/value/kind"),
        Some(&json!("replayed")),
        "{replay}"
    );
    assert_eq!(
        replay.pointer("/value/value/value/status/kind"),
        Some(&json!("sent")),
        "{replay}"
    );
    assert_eq!(
        admitted_rows(&fixture).await?,
        before,
        "replaying existing custody during revocation added work"
    );
    let candidate = envelope(B, SERVICE_B)?;
    let refused = submit_answer(&fixture, &candidate).await?;
    assert_eq!(
        refused.pointer("/value/value/kind"),
        Some(&json!("refused")),
        "{refused}"
    );
    assert_eq!(
        refused.pointer("/value/value/value/reason/kind"),
        Some(&json!("recipientRevoked")),
        "{refused}"
    );
    assert_eq!(
        admitted_rows(&fixture).await?,
        before,
        "refused new mail changed outbox rows or Push tasks"
    );
    assert!(matches!(
        status(&fixture, candidate.id()).await?.state,
        ItemStatusState::Unknown
    ));
    assert_eq!(
        custody(&fixture, earlier.id()).await?,
        ("sent".into(), true)
    );
    fixture.close().await
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
async fn a_revoked_recipient_is_never_resent_its_push_and_fleet_keeps_sent_content() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let before = custody(&fixture, item.id()).await?;
    assert_eq!(before, ("sent".into(), true));
    // A connected recipient is sent its push at once (owed-work design).
    recipient.push(item.id()).await?;
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
            let ended = fixture.runtime.conformance_drive_operations().await?;
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
        // Past its retry time the unanswered push is due, but the revoked
        // machine has no connection: nothing is resent and custody stays.
        fixture.io.advance(40_000);
        crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
        let driven = fixture.runtime.conformance_drive_operations().await?;
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
    fixture.runtime.conformance_drive_operations().await?;
    let deadline = fleet_mail_delivery::removal_deadline(item.id()).unix_millis();
    fixture.io.advance(deadline - NOW - 1);
    // Retention is a due source: the alarm runs it (slice 4b), not a journal task.
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert!(matches!(
        status(&fixture, item.id()).await?.state,
        ItemStatusState::Known {
            status: DeliveryStatus::Sent,
            ..
        }
    ));
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    fixture.io.advance(1);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
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
