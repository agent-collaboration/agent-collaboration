//! Clean move and takeover preserve the literal debt observed by the shared fixture.
pub use super::follow_fixture::{
    FollowMembership, delivered, native_state, only_item, sender_status,
};
use super::{
    central_pair::{SecondHost, await_writer, central, settled_view},
    fixture::{BoardFixture, at, fields, identity},
    takeover_controls::{self as controls, BOUND},
    takeover_readbacks as readbacks,
    takeover_recipients::{self as recipients, Recipients},
};
use board_central_test_support::{DispatchCallSelector, ObjectAddress};
use board_sync_protocol::BoardDeliveryTypes;
use codex_router_test_support::FixtureResult;
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ReplicationPosition};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub enum TransferKind {
    Move,
    Takeover,
}

pub async fn transfer(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    kind: TransferKind,
) -> FixtureResult {
    recipients::scenario(fixture, second, async move |fixture, second, peers| {
        Box::pin(controls::scenario(
            fixture,
            second,
            async move |fixture, second| prove_transfer(fixture, second, peers, kind).await,
        ))
        .await
    })
    .await
}

async fn prove_transfer(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    peers: &mut Recipients,
    kind: TransferKind,
) -> FixtureResult {
    fixture.create().await?;
    let follow =
        FollowMembership::arrange(fixture, recipients::session(second.host()?).await?).await?;
    settled_view(fixture.host()?, &fixture.project).await?;
    controls::hold_selected(
        fixture,
        controls::connection(fixture)?,
        ObjectAddress::Registry,
        DispatchCallSelector::InstallationPeer,
    )
    .await?;
    let first = follow.post(fixture, "first inherited owed post").await?;
    let last = follow.post(fixture, "second inherited owed post").await?;
    let range = fields(
        first,
        json!({"through":at(&last, "/through")?,"postCount":2}),
    )?;
    let lookup = controls::held_call(
        fixture,
        controls::connection(fixture)?,
        "central.installation-peer",
    )
    .await?;
    assert_eq!(
        lookup.get("kind"),
        Some(&json!("central.installation-peer"))
    );
    settled_view(fixture.host()?, &fixture.project).await?;
    controls::confirm_prefix(fixture, fixture.host()?, at(&range, "/through")?).await?;
    let copied = controls::copy_ack(fixture).await?;
    transfer_unallocated_range(fixture, second, kind, &range, copied).await?;
    peers.start_second(second, 1).await?;
    let recipient_line = peers
        .second()?
        .expect_text_with_timeout("router://", BOUND)
        .await?;
    let stopped = second.stop_for_readback().await?;
    let original = only_item(readbacks::deliveries(
        readbacks::handoffs(&stopped, &fixture.project, 2).await?,
    ))?;
    follow.assert_frozen(&original, &range)?;
    assert!(recipient_line.contains(&original.id().as_uuid().to_string()));
    readbacks::assert_no_wake(&stopped).await?;
    drop(stopped);
    second.reopen_host().await?;
    let shown = delivered(
        second.host()?,
        &follow.target,
        &original.id().as_uuid().to_string(),
    )
    .await?;
    assert_eq!(
        shown.record.push_id.as_str(),
        original.id().as_uuid().to_string()
    );
    let stopped = second.stop_for_readback().await?;
    let inherited = only_item(readbacks::deliveries(
        readbacks::handoffs(&stopped, &fixture.project, 2).await?,
    ))?;
    assert_eq!(
        serde_json::to_value(&inherited)?,
        serde_json::to_value(&original)?
    );
    follow.assert_frozen(&inherited, &range)?;
    readbacks::assert_no_wake(&stopped).await?;
    assert_eq!(peers.second()?.count_after_barrier().await?, 1);
    Ok(())
}

async fn transfer_unallocated_range(
    fixture: &mut BoardFixture,
    second: &SecondHost,
    kind: TransferKind,
    range: &Value,
    copied: ReplicationPosition,
) -> FixtureResult {
    match kind {
        TransferKind::Takeover => {
            let stopped = fixture
                .host
                .take()
                .ok_or("predecessor Host absent")?
                .crash_for_readback()
                .await?;
            assert_unallocated(&stopped, fixture, range, copied).await?;
            drop(stopped);
            transfer_to_second(fixture, second, kind).await?;
            controls::resume(fixture, controls::connection(fixture)?).await?;
        }
        TransferKind::Move => {
            transfer_to_second(fixture, second, kind).await?;
            controls::resume(fixture, controls::connection(fixture)?).await?;
            let stopped = fixture
                .host
                .take()
                .ok_or("predecessor Host absent")?
                .stop_for_readback()
                .await?;
            assert_unallocated(&stopped, fixture, range, copied).await?;
        }
    }
    Ok(())
}

async fn assert_unallocated(
    stopped: &super::owned_host::StoppedHost,
    fixture: &BoardFixture,
    range: &Value,
    copied: ReplicationPosition,
) -> FixtureResult {
    assert!(
        readbacks::deliveries(readbacks::handoffs(stopped, &fixture.project, 1).await?).is_empty(),
        "owed range was allocated before transfer"
    );
    let through = serde_json::from_value(at(range, "/through")?.clone())?;
    let cause = readbacks::cause_capture(stopped, &fixture.project, through).await?;
    assert_eq!(copied.epoch, cause.epoch);
    assert!(copied.sequence >= cause.sequence);
    eprintln!(
        "T owed prefix evidence: {}",
        json!({"cause":cause,"copyAck":copied,"range":range,"allocatedBeforeTransfer":false})
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
pub fn submitted_item(request: &Value) -> FixtureResult<DeliveryItem<BoardDeliveryTypes>> {
    assert_eq!(request.get("kind"), Some(&json!("central.mailbox-submit")));
    let envelope: DeliveryEnvelope<BoardDeliveryTypes> =
        serde_json::from_value(at(request, "/request/item")?.clone())?;
    let DeliveryEnvelope::Item(item) = envelope else {
        return Err("expected actual submitted follow item".into());
    };
    Ok(item)
}

pub async fn transfer_to_second(
    fixture: &mut BoardFixture,
    second: &SecondHost,
    kind: TransferKind,
) -> FixtureResult {
    let target = second.service_id()?.try_into()?;
    let database = fixture.project.clone().try_into()?;
    let transfer = identity().try_into()?;
    let reply = match kind {
        TransferKind::Move => {
            central(fixture)?
                .move_project(database, &target, transfer)
                .await?
        }
        TransferKind::Takeover => {
            if let Some(host) = fixture.host.take() {
                drop(host.crash_for_readback().await?);
            }
            central(fixture)?
                .take_over_project(database, &target, transfer)
                .await?
        }
    };
    assert_eq!(reply.pointer("/value/newEpoch"), Some(&json!(2)));
    await_writer(second.host()?, &fixture.project, 2).await?;
    Ok(())
}
