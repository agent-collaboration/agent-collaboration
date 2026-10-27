//! Real owner takeover and acknowledged prefixes decide handoff recovery.
#![deny(clippy::too_many_lines)]
use super::*;
use board_central_test_support::{ObjectAddress, RuntimeControl};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_contracts::ProjectReplicationPosition;
use fleet_sync_protocol::{DatabaseId, DeliveryItem, ItemId, TransferId};
use serde_json::json;

#[path = "messaging_cancel_cases.rs"]
mod cancel_cases;
#[path = "messaging_local_cancel_case.rs"]
mod local_cancel_case;
#[path = "../../../../../collaboration-central-tokio/tests/support/mail_relay_transport.rs"]
mod relay_transport;
#[path = "messaging_rule_fixture.rs"]
mod rule_fixture;
use rule_fixture::*;

#[tokio::test]
async fn takeover_after_replicated_handoff_publishes_one_original_item() -> TestResult {
    prove_takeover(PrefixCase::Both).await
}
#[tokio::test]
async fn takeover_without_replicated_cause_or_handoff_drops_the_lost_tail() -> TestResult {
    prove_takeover(PrefixCase::Neither).await
}
#[tokio::test]
async fn takeover_with_only_replicated_cause_freezes_one_successor_item() -> TestResult {
    prove_takeover(PrefixCase::CauseOnly).await
}

#[derive(Clone, Copy)]
enum PrefixCase {
    Neither,
    CauseOnly,
    Both,
}

async fn prove_takeover(case: PrefixCase) -> TestResult {
    let mut fixture = RuleFixture::open(!matches!(case, PrefixCase::Both)).await?;
    let proof = async {
        let progress_hold = match fixture.holder.notifications.as_ref() {
            Some(owner) => Some(owner.hold_progress_writer().await?),
            None => None,
        };
        let copy = ObjectAddress::Copy(DatabaseId::try_from(fixture.project.as_str().to_owned())?);
        let connection = ObjectAddress::Connection(origin()?.installation);
        if matches!(case, PrefixCase::Neither) {
            fixture
                .holder
                .central
                .runtime_control(
                    connection.clone(),
                    RuntimeControl::HoldNextDispatch {
                        destination: copy.clone(),
                    },
                )
                .await?;
        }
        let post = fixture.post_cause().await?;
        let cause = fixture.head().await?;
        if !matches!(case, PrefixCase::Neither) {
            fixture.confirm_prefix(cause).await?;
        }
        if matches!(case, PrefixCase::CauseOnly) {
            fixture
                .holder
                .central
                .runtime_control(
                    connection.clone(),
                    RuntimeControl::HoldNextDispatch { destination: copy },
                )
                .await?;
        }
        let original = fixture.freeze(&post).await?;
        let handoff = fixture.head().await?;
        if matches!(case, PrefixCase::Both) {
            fixture.confirm_prefix(handoff).await?;
        } else {
            let held = fixture
                .holder
                .central
                .runtime_control(connection.clone(), RuntimeControl::AwaitHeldDispatch)
                .await?;
            assert!(matches!(
                held.dispatch,
                board_central_test_support::DispatchFaultObservation::Held { .. }
            ));
            assert!(
                fixture
                    .holder
                    .projects
                    .replicated_through(&fixture.project, handoff.epoch)
                    .await?
                    .position
                    < handoff.position
            );
        }
        if let Some((release, task)) = progress_hold {
            release.send(()).map_err(|_| "old publication hold ended")?;
            task.await??;
            assert_preacknowledgement_custody_absent(&fixture).await?;
        }
        take_over_and_confirm(&mut fixture, case, post, original).await
    }
    .await;
    let closed = fixture.close().await;
    closed?;
    proof
}

async fn take_over_and_confirm(
    fixture: &mut RuleFixture,
    case: PrefixCase,
    post: PostId,
    original: DeliveryItem<BoardDeliveryTypes>,
) -> TestResult {
    fixture.holder.disconnect_holder().await?;
    let moved = fixture
        .holder
        .central
        .take_over_project(
            DatabaseId::try_from(fixture.project.as_str().to_owned())?,
            &fixture.successor_origin.installation,
            TransferId::new(durable_steps::Uuid::now_v7())?,
        )
        .await?;
    assert_eq!(moved.pointer("/value/newEpoch"), Some(&json!(3)));
    if !matches!(case, PrefixCase::Both) {
        fixture
            .holder
            .central
            .runtime_control(
                ObjectAddress::Connection(origin()?.installation),
                RuntimeControl::ResumeDispatch,
            )
            .await?;
    }
    let store =
        received_grants::wait_published_project(&fixture.successor.registry, &fixture.project, 3)
            .await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.record(&post.into()).await?.is_some(),
        !matches!(case, PrefixCase::Neither)
    );
    read.finish().await?;
    fixture.assert_successor_publication(case, &original).await
}

async fn assert_preacknowledgement_custody_absent(fixture: &RuleFixture) -> TestResult {
    let owner = fixture
        .holder
        .notifications
        .as_ref()
        .ok_or("old publisher")?;
    owner
        .publisher
        .queue_project(fixture.project.clone())
        .await?;
    timeout(
        BOUND,
        owner.publisher.wait_project_idle_for_test(&fixture.project),
    )
    .await?;
    assert!(
        owner
            .rows()
            .await?
            .iter()
            .all(|row| row.category != "outbox"),
        "unacknowledged handoff entered native outgoing custody"
    );
    Ok(())
}
