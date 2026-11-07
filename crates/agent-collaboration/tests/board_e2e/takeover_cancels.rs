//! MS-9 preserves prevention, discarded ends and frozen child-cancel identities.
use super::{
    central_pair::{SecondHost, settled_view},
    fixture::{BoardFixture, at},
    takeover_controls::{self as controls, BOUND, EndPrefix},
    takeover_follows::{
        FollowMembership, TransferKind, native_state, only_item, sender_status, submitted_item,
        transfer_to_second,
    },
    takeover_readbacks as readbacks,
    takeover_recipients::{self as recipients, Recipients},
};
use board_central_test_support::{DispatchCallSelector, ObjectAddress, RuntimeControl};
use board_sync_protocol::BoardDeliveryTypes;
use codex_router_test_support::FixtureResult;
use collaboration_protocol::PushDeliveryState;
use fleet_protocol::{DeliveryEnvelope, DeliveryItem};
use serde_json::{Value, json};

#[path = "takeover_copy_progress.rs"]
mod copy_progress;
use copy_progress::copy_project;
#[path = "takeover_frontier_cut.rs"]
mod frontier_cut;
use frontier_cut::{EndCutConsequence, RemoteEndCut};
pub use frontier_cut::{FrontierCut, RemoteEndPrefix};
#[path = "takeover_cancel_successor.rs"]
mod cancel_successor;
#[path = "takeover_local_prevention.rs"]
mod local_prevention;
pub use local_prevention::held_local;

struct HeldFollow {
    follow: FollowMembership,
    item: DeliveryItem<BoardDeliveryTypes>,
    range: Value,
}

async fn held_remote(fixture: &mut BoardFixture, second: &SecondHost) -> FixtureResult<HeldFollow> {
    fixture.create().await?;
    let follow =
        FollowMembership::arrange(fixture, recipients::session(second.host()?).await?).await?;
    copy_project(fixture).await?;
    settled_view(fixture.host()?, &fixture.project).await?;
    controls::hold_selected(
        fixture,
        ObjectAddress::Fleet,
        ObjectAddress::Fleet,
        DispatchCallSelector::FleetMailSubmit,
    )
    .await?;
    let range = follow
        .post(fixture, "held follow cancellation range")
        .await?;
    let item = submitted_item(&copy_progress::held_submit(fixture).await?)?;
    follow.assert_frozen(&item, &range)?;
    copy_project(fixture).await?;
    settled_view(fixture.host()?, &fixture.project).await?;
    controls::resume(fixture, ObjectAddress::Fleet, controls::submit_key()).await?;
    sender_status(fixture.host()?, &item.id().as_uuid().to_string(), "held").await?;
    native_state(
        second.host()?,
        &follow.target,
        &item.id().as_uuid().to_string(),
        PushDeliveryState::Held,
    )
    .await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("predecessor Host absent")?
        .stop_for_readback()
        .await?;
    let persisted = only_item(readbacks::deliveries(
        readbacks::handoffs(&stopped, &fixture.project, 1).await?,
    ))?;
    assert_eq!(
        serde_json::to_value(&persisted)?,
        serde_json::to_value(&item)?
    );
    let captured = readbacks::handoff_capture(&stopped, &fixture.project, item.id()).await?;
    drop(stopped);
    fixture.reopen_host().await?;
    copy_project(fixture).await?;
    controls::confirm_copy_ack(fixture, captured).await?;
    Ok(HeldFollow {
        follow,
        item,
        range,
    })
}

pub async fn remote(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    prefix: RemoteEndPrefix,
) -> FixtureResult {
    recipients::scenario(fixture, second, async move |fixture, second, peers| {
        Box::pin(controls::scenario(
            fixture,
            second,
            async move |fixture, second| prove_remote(fixture, second, peers, prefix).await,
        ))
        .await
    })
    .await
}

async fn prove_remote(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    peers: &mut Recipients,
    remote_prefix: RemoteEndPrefix,
) -> FixtureResult {
    let prefix = remote_prefix.end_prefix();
    let seed = held_remote(fixture, second).await?;
    let cut = arrange_remote_end(fixture, second, &seed, remote_prefix).await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("predecessor Host absent")?
        .crash_for_readback()
        .await?;
    let before = readbacks::handoffs(&stopped, &fixture.project, 1).await?;
    let [
        message_board_storage::MailHandoffItem::CancelGroup(_),
        message_board_storage::MailHandoffItem::Delivery(original),
        message_board_storage::MailHandoffItem::CancelGroup(_),
    ] = before.as_slice()
    else {
        return Err("remote-end fixture must retain policy1, original2 and end3".into());
    };
    assert_eq!(original.id(), seed.item.id());
    assert_end_groups(&before, &seed.follow, true)?;
    let expansions = readbacks::cancel_expansions(&stopped, &fixture.project, 1).await?;
    if !matches!(prefix, EndPrefix::Discarded) {
        let [expansion] = expansions.as_slice() else {
            return Err("retained end has no unique frozen child cancel".into());
        };
        assert_eq!(expansion.get("target"), Some(&json!(seed.item.id())));
        if let EndCutConsequence::LostReceipt(lost) = &cut.consequence {
            assert_eq!(expansion.get("envelope"), Some(lost));
        }
    }
    controls::assert_end_cut(
        fixture,
        &stopped,
        seed.item.id(),
        cut.end,
        prefix,
        &expansions,
        cut.pre_end_ack,
    )
    .await?;
    if let EndCutConsequence::Retained { cut, frontier } = cut.consequence {
        let copied = controls::copy_ack(fixture).await?;
        assert_eq!(copied.epoch, frontier.position.epoch);
        match cut {
            FrontierCut::Unsettled => assert!(
                copied.sequence < frontier.position.sequence,
                "unsettled cut copied the frontier"
            ),
            FrontierCut::Settled => assert!(
                copied.sequence >= frontier.position.sequence,
                "settled cut omitted the frontier"
            ),
        }
        match cut {
            FrontierCut::Unsettled => assert!(
                frontier.settled_through.get() >= 2,
                "held frontier did not cover the original"
            ),
            FrontierCut::Settled => assert_eq!(
                frontier.settled_through.get(),
                3,
                "settled cut omitted the end group"
            ),
        }
        eprintln!(
            "CI6 frontier prefix: {cut:?}, captured={:?}, copyAck={copied:?}",
            frontier.position
        );
    }
    drop(stopped);
    recipient_before_transfer(second, &seed, prefix).await?;
    transfer_to_second(fixture, second, TransferKind::Takeover)
        .await
        .map_err(|error| format!("CI6 successor writer activation: {error}"))?;
    release_predecessor_frontier(fixture, prefix).await?;
    cancel_successor::assert_remote_successor(
        fixture,
        second,
        peers,
        remote_prefix,
        &seed,
        expansions,
    )
    .await
}

async fn release_predecessor_frontier(fixture: &BoardFixture, prefix: EndPrefix) -> FixtureResult {
    if matches!(prefix, EndPrefix::Discarded | EndPrefix::Retained) {
        // The held frontier belongs to the dead predecessor; release its
        // conformance dispatch only after the successor seed is installed.
        let observed = super::central_pair::central(fixture)?
            .runtime_control(
                board_central_test_support::ObjectAddress::Fleet,
                RuntimeControl::Observe,
            )
            .await?;
        if matches!(
            observed.dispatch(&controls::copy_write_key(fixture)?),
            board_central_test_support::DispatchFaultObservation::Held { .. }
        ) {
            controls::resume(
                fixture,
                board_central_test_support::ObjectAddress::Fleet,
                controls::copy_write_key(fixture)?,
            )
            .await?;
        }
    }
    Ok(())
}

async fn arrange_remote_end(
    fixture: &mut BoardFixture,
    second: &SecondHost,
    seed: &HeldFollow,
    remote_prefix: RemoteEndPrefix,
) -> FixtureResult<RemoteEndCut> {
    let prefix = remote_prefix.end_prefix();
    let fleet = ObjectAddress::Fleet;
    match prefix {
        EndPrefix::Discarded => {
            controls::hold_selected(
                fixture,
                board_central_test_support::ObjectAddress::Fleet,
                controls::copy(fixture)?,
                DispatchCallSelector::CopyWrite,
            )
            .await?
        }
        EndPrefix::LostReceipt => {
            controls::hold_selected(
                fixture,
                ObjectAddress::Fleet,
                fleet.clone(),
                DispatchCallSelector::FleetMailSubmit,
            )
            .await?
        }
        EndPrefix::Retained => {
            controls::hold_selected(
                fixture,
                board_central_test_support::ObjectAddress::Fleet,
                controls::copy(fixture)?,
                DispatchCallSelector::CopyWrite,
            )
            .await?;
        }
    }
    let pre_end_ack = controls::copy_ack(fixture).await?;
    let end = seed.follow.unsubscribe(fixture).await?;
    let consequence = match prefix {
        EndPrefix::Discarded => {
            let held = controls::held(
                fixture,
                board_central_test_support::ObjectAddress::Fleet,
                controls::copy_write_key(fixture)?,
            )
            .await?;
            assert_eq!(held.get("kind"), Some(&json!("central.copy-session-write")));
            native_state(
                second.host()?,
                &seed.follow.target,
                &seed.item.id().as_uuid().to_string(),
                PushDeliveryState::Held,
            )
            .await?;
            EndCutConsequence::Discarded
        }
        EndPrefix::Retained => {
            let RemoteEndPrefix::Retained(cut) = remote_prefix else {
                return Err("retained end has no frontier cut".into());
            };
            let frontier = frontier_cut::establish(fixture, cut).await?;
            sender_status(
                fixture.host()?,
                &seed.item.id().as_uuid().to_string(),
                "cancelled",
            )
            .await
            .map_err(|error| format!("CI6 retained-end original cancellation: {error}"))?;
            settled_view(fixture.host()?, &fixture.project).await?;
            EndCutConsequence::Retained { cut, frontier }
        }
        EndPrefix::LostReceipt => EndCutConsequence::LostReceipt(
            lose_cancel_answer(fixture, fleet, seed.item.id().as_uuid().to_string()).await?,
        ),
    };
    Ok(RemoteEndCut {
        pre_end_ack,
        end,
        consequence,
    })
}

async fn lose_cancel_answer(
    fixture: &BoardFixture,
    fleet: ObjectAddress,
    original: String,
) -> FixtureResult<Value> {
    tokio::time::timeout(BOUND, async {
        loop {
            let request =
                controls::held(fixture, ObjectAddress::Fleet, controls::submit_key()).await?;
            if request.get("kind") == Some(&json!("central.mail-submit")) {
                let item: DeliveryEnvelope<BoardDeliveryTypes> =
                    serde_json::from_value(at(&request, "/request/item")?.clone())?;
                if let DeliveryEnvelope::Cancel(cancel) = item {
                    assert_eq!(cancel.target().as_uuid().to_string(), original);
                    let dropped = controls::release_and_drop_selected_reply(
                        fixture,
                        ObjectAddress::Fleet,
                        fleet.clone(),
                        DispatchCallSelector::FleetMailSubmit,
                    )
                    .await?;
                    assert_eq!(dropped, request);
                    return Ok(at(&request, "/request/item")?.clone());
                }
            }
            super::central_pair::central(fixture)?
                .runtime_control(
                    ObjectAddress::Fleet,
                    RuntimeControl::ResumeDispatchAndHoldNext {
                        key: controls::submit_key(),
                        next: controls::submit_key(),
                    },
                )
                .await?;
        }
    })
    .await?
}

async fn recipient_before_transfer(
    second: &mut SecondHost,
    seed: &HeldFollow,
    prefix: EndPrefix,
) -> FixtureResult {
    let expected = match prefix {
        EndPrefix::Discarded => PushDeliveryState::Held,
        EndPrefix::Retained | EndPrefix::LostReceipt => PushDeliveryState::Rejected,
    };
    native_state(
        second.host()?,
        &seed.follow.target,
        &seed.item.id().as_uuid().to_string(),
        expected,
    )
    .await?;
    let stopped = second.stop_for_readback().await?;
    assert!(
        readbacks::local_cancel_deliveries(&stopped)
            .await?
            .is_empty(),
        "remote receive already has Local cancellation attribution before takeover"
    );
    drop(stopped);
    second.reopen_host().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
fn assert_end_groups(
    handoffs: &[message_board_storage::MailHandoffItem],
    follow: &FollowMembership,
    ended: bool,
) -> FixtureResult {
    use message_board_storage::{MailCancelGroupCause, MailHandoffItem};
    let groups = handoffs
        .iter()
        .filter_map(|row| match row {
            MailHandoffItem::CancelGroup(group) => Some(group),
            MailHandoffItem::Delivery(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(groups.len(), 1 + usize::from(ended));
    for group in &groups {
        assert_eq!(
            group.membership.as_str(),
            follow.membership.envelope.id.as_str()
        );
        assert_eq!(group.activated_at, follow.membership.activated_at());
    }
    let prior = groups.first().ok_or("policy-change cancel group missing")?;
    assert_eq!(prior.generation.get(), 1);
    assert_eq!(prior.cause, MailCancelGroupCause::GenerationChanged);
    if ended {
        let end = groups
            .get(1)
            .ok_or("subscription-end cancel group missing")?;
        assert_eq!(end.generation.get(), 2);
        assert_eq!(
            end.cause,
            MailCancelGroupCause::MembershipEnded {
                reason: message_board::EndReason::Cancelled,
            }
        );
    }
    Ok(())
}
