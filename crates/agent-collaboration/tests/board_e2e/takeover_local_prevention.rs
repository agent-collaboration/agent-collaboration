//! Preserve local prevention when the successor never receives the subscription end.
use super::super::{
    central_pair::{SecondHost, central, settled_view},
    fixture::BoardFixture,
    takeover_controls::{self as controls, BOUND, EndPrefix},
    takeover_follows::{
        FollowMembership, TransferKind, native_state, only_item, sender_status, transfer_to_second,
    },
    takeover_readbacks as readbacks,
    takeover_recipients::{self as recipients, Recipients},
};
use super::{HeldFollow, assert_end_groups};
use board_central_test_support::DispatchCallSelector;
use board_sync_protocol::BoardDeliveryTypes;
use codex_router_test_support::FixtureResult;
use collaboration_protocol::PushDeliveryState;
use fleet_sync_protocol::{DeliveryItem, ItemId, ReportSeq};
use serde_json::json;

struct LocalPrevention {
    held_sequence: ReportSeq,
    cancel: ItemId,
}

pub async fn held_local(fixture: &mut BoardFixture, second: &mut SecondHost) -> FixtureResult {
    recipients::scenario(fixture, second, async |fixture, second, peers| {
        Box::pin(controls::scenario(
            fixture,
            second,
            async |fixture, second| prove_local(fixture, second, peers).await,
        ))
        .await
    })
    .await
}

async fn prove_local(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    peers: &mut Recipients,
) -> FixtureResult {
    let mut wire = central(fixture)?.observe_mail_wire(None)?;
    let mut status_wire = second.enrolled_machine()?.observe_mail_wire(None)?;
    // Arrange the existing test-only connected-status cadence before any transfer.
    second.stop_host().await?;
    std::fs::write(
        second.directory.join("fixture-mail-status-interval-ms"),
        "2000",
    )?;
    second.reopen_host().await?;
    let (HeldFollow { follow, item, .. }, held_sequence) = held_local_follow(fixture).await?;
    controls::hold_selected(
        fixture,
        collaboration_central_conformance::legacy_fleet_address(),
        controls::copy(fixture)?,
        DispatchCallSelector::CopyWrite,
    )
    .await?;
    let pre_end_ack = controls::copy_ack(fixture).await?;
    let end = follow.unsubscribe(fixture).await?;
    controls::held(
        fixture,
        collaboration_central_conformance::legacy_fleet_address(),
        controls::copy_write_key(fixture)?,
    )
    .await?;
    let rejected = native_state(
        fixture.host()?,
        &follow.target,
        &item.id().as_uuid().to_string(),
        PushDeliveryState::Rejected,
    )
    .await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("predecessor Host absent")?
        .crash_for_readback()
        .await?;
    let old = readbacks::handoffs(&stopped, &fixture.project, 1).await?;
    assert_end_groups(&old, &follow, true)?;
    let expected_cancel = readbacks::local_prevention_cancel(&stopped, item.id()).await?;
    controls::assert_end_cut(
        fixture,
        &stopped,
        item.id(),
        end,
        EndPrefix::Discarded,
        &[],
        pre_end_ack,
    )
    .await?;
    drop(stopped);
    transfer_to_second(fixture, second, TransferKind::Takeover).await?;
    controls::resume(
        fixture,
        collaboration_central_conformance::legacy_fleet_address(),
        controls::copy_write_key(fixture)?,
    )
    .await?;
    fixture.reopen_host().await?;
    peers.start_first(fixture, 0).await?;
    sender_status(
        second.host()?,
        &item.id().as_uuid().to_string(),
        "cancelled",
    )
    .await?;
    let cancel = assert_local_prevention(
        fixture,
        &follow,
        &item,
        &rejected,
        &mut wire,
        &mut status_wire,
        LocalPrevention {
            held_sequence,
            cancel: expected_cancel,
        },
    )
    .await?;
    sender_status(
        second.host()?,
        &item.id().as_uuid().to_string(),
        "cancelled",
    )
    .await?;
    assert_final_local_custody(fixture, second, &follow, &item, cancel, held_sequence).await?;
    assert_eq!(peers.first()?.count_after_barrier().await?, 0);
    assert_eq!(controls::report_count(&wire, item.id()), 1);
    eprintln!(
        "T held-local containment evidence: {}",
        json!({"item":item.id(),"heldOutcomeSequence":held_sequence.value(),"preventedOutcomeSequence":held_sequence.value()+1,"promotionReportSequence":held_sequence.value()+2,"cancel":cancel,"typedReplayedPush":true,"reportSends":1,"minimumFutureStatusScans":6,"recipientReopened":true,"nativeFinal":"prevented","runningReport":false,"senderStatus":"cancelled"})
    );
    Ok(())
}

async fn assert_final_local_custody(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    follow: &FollowMembership,
    item: &DeliveryItem<BoardDeliveryTypes>,
    cancel: ItemId,
    held_sequence: ReportSeq,
) -> FixtureResult {
    let stopped = second.stop_for_readback().await?;
    readbacks::assert_single_cancelled_sender(&stopped, item.id()).await?;
    let inherited = readbacks::handoffs(&stopped, &fixture.project, 2).await?;
    readbacks::assert_ongoing_membership(&stopped, &fixture.project, 2, &follow.membership).await?;
    assert_end_groups(&inherited, follow, false)?;
    let actual = only_item(readbacks::deliveries(inherited))?;
    assert_eq!(serde_json::to_value(actual)?, serde_json::to_value(item)?);
    readbacks::assert_no_wake(&stopped).await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("recipient Host absent")?
        .stop_for_readback()
        .await?;
    readbacks::assert_no_wake(&stopped).await?;
    assert_eq!(
        readbacks::prevented_cancel_stopped(&stopped, item.id(), held_sequence).await?,
        cancel
    );
    Ok(())
}

async fn assert_local_prevention(
    fixture: &mut BoardFixture,
    follow: &FollowMembership,
    item: &DeliveryItem<BoardDeliveryTypes>,
    rejected: &collaboration_protocol::PushRecordShowResult,
    wire: &mut board_central_test_support::MailWireObservation,
    status_wire: &mut board_central_test_support::MailWireObservation,
    prevention: LocalPrevention,
) -> FixtureResult<fleet_sync_protocol::ItemId> {
    let LocalPrevention {
        held_sequence,
        cancel: expected_cancel,
    } = prevention;
    let promoted_sequence = held_sequence
        .next()
        .and_then(|sequence| sequence.next())
        .ok_or("promotion report sequence exhausted")?;
    controls::assert_prevented_push_replayed(wire, item.id(), expected_cancel, promoted_sequence)
        .await?;
    controls::assert_single_promoted_report(wire, status_wire, item.id())
        .await
        .map_err(|error| format!("before recipient reopen: {error}"))?;
    let stopped = fixture
        .host
        .take()
        .ok_or("recipient Host absent")?
        .stop_for_readback()
        .await?;
    let cancel = readbacks::prevented_cancel_stopped(&stopped, item.id(), held_sequence).await?;
    drop(stopped);
    fixture.reopen_host().await?;
    let retained = native_state(
        fixture.host()?,
        &follow.target,
        &item.id().as_uuid().to_string(),
        PushDeliveryState::Rejected,
    )
    .await
    .map_err(|error| format!("recipient prevention after reopen: {error}"))?;
    assert_eq!(retained.record.push_id, rejected.record.push_id);
    assert_eq!(
        serde_json::to_value(retained.record.last_outcome)?,
        serde_json::to_value(&rejected.record.last_outcome)?
    );
    controls::assert_single_promoted_report(wire, status_wire, item.id())
        .await
        .map_err(|error| format!("after recipient reopen: {error}"))?;
    Ok(cancel)
}

async fn held_local_follow(
    fixture: &mut BoardFixture,
) -> FixtureResult<(HeldFollow, fleet_sync_protocol::ReportSeq)> {
    fixture.create().await?;
    let follow =
        FollowMembership::arrange(fixture, recipients::session(fixture.host()?).await?).await?;
    let range = follow
        .post(fixture, "held local item survives discarded end")
        .await?;
    await_pending(fixture, &follow).await?;
    settled_view(fixture.host()?, &fixture.project).await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("predecessor Host absent")?
        .stop_for_readback()
        .await?;
    let item = only_item(readbacks::deliveries(
        readbacks::handoffs(&stopped, &fixture.project, 1).await?,
    ))?;
    follow.assert_frozen(&item, &range)?;
    let held_sequence = readbacks::held_local_sequence_stopped(&stopped, item.id()).await?;
    drop(stopped);
    fixture.reopen_host().await?;
    let captured = controls::replicate_local_handoff(fixture, &item).await?;
    controls::confirm_copy_ack(fixture, captured).await?;
    native_state(
        fixture.host()?,
        &follow.target,
        &item.id().as_uuid().to_string(),
        PushDeliveryState::Held,
    )
    .await?;
    Ok((
        HeldFollow {
            follow,
            item,
            range,
        },
        held_sequence,
    ))
}

async fn await_pending(fixture: &BoardFixture, follow: &FollowMembership) -> FixtureResult {
    tokio::time::timeout(BOUND, async {
        loop {
            let view = fixture
                .host()?
                .mcp(
                    "thread_subscriptions",
                    json!({"actor":{
                "kind":"session","session":follow.target}}),
                )
                .await?;
            if view.pointer("/subscriptions/0/pendingCount") == Some(&json!(1)) {
                assert!(view.pointer("/subscriptions/0/heldSince").is_some());
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
