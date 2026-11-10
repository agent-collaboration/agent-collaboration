//! Row16 process case: a real held follow survives actual native history pruning.
#![deny(clippy::too_many_lines)]
use super::{
    acp_recipient::AcpRecipient,
    fixture::{BoardFixture, identity},
    follow_fixture::{FollowMembership, delivered, native_state, only_item},
    takeover_readbacks::{deliveries, handoffs},
};
use codex_router_test_support::FixtureResult;
use collaboration_protocol::{PushDeliveryState, PushRecord, PushRecordShowResult};
use owned_process_test_support::scenario_finalization::run_scenario;
use serde_json::json;

const CONTROL_BODY: &str = "row16 independent terminal history pruning control";
const FOLLOW_SUMMARY: &str = "Router: new thread activity";

pub async fn held_follow_survives_history_pruning(fixture: &mut BoardFixture) -> FixtureResult {
    fixture.create().await?;
    let installation = fixture.installation.as_ref().ok_or("installation absent")?;
    // The recipient is a session the Router holds for while it is not loaded and later
    // loads: a Cursor-endpoint session on the fixture provider this Host now runs.
    let recipient = AcpRecipient::arm(
        &fixture.directory,
        &String::from(installation.service_id.clone()),
        &identity(),
    )?;
    fixture.stop_host().await?;
    fixture.reopen_host().await?;
    let target = recipient.target.clone();
    // Present for the control message: the provider loads the session, which also gives
    // the Router its record, so later it can tell a held session from an unknown one.
    recipient.load(fixture.host()?).await?;
    let control = terminal_control(fixture, &recipient).await?;
    // The recipient goes absent: a fresh Host's provider has not loaded its session.
    fixture.stop_host().await?;
    fixture.reopen_host().await?;
    let held = hold_and_prune(fixture, &target, &control).await?;
    // It comes back: the provider loads the session, and only that load releases the follow.
    recipient.come_back(fixture.host()?).await?;
    deliver_after_prune(fixture, &recipient, &control, &held).await
}

async fn hold_and_prune(
    fixture: &mut BoardFixture,
    target: &collaboration_protocol::SessionRef,
    control: &PushRecord,
) -> FixtureResult<PushRecord> {
    // The reader is declared but now truly absent; whenIdle=hold preserves its batch.
    let follow = FollowMembership::arrange(fixture, target.clone()).await?;
    let range = follow
        .post(fixture, "row16 complete held follow cause")
        .await?;
    let snapshot = fixture
        .host()?
        .mcp(
            "thread_subscriptions",
            json!({"actor":{"kind":"session","session":target}}),
        )
        .await?;
    assert_eq!(snapshot.get("unavailable"), Some(&json!([])));
    let stopped = fixture
        .host
        .take()
        .ok_or("held readback Host absent")?
        .stop_for_readback()
        .await?;
    let recipient: message_board::SessionRef =
        serde_json::from_value(serde_json::to_value(target)?)?;
    let items = deliveries(handoffs(&stopped, &fixture.project, 1).await?)
        .into_iter()
        .filter(|item| item.recipient().address == recipient)
        .collect();
    let item = only_item(items)?;
    follow.assert_frozen(&item, &range)?;
    let item_id = item.id().as_uuid().to_string();
    fixture.reopen_host().await?;
    let held = native_state(fixture.host()?, target, &item_id, PushDeliveryState::Held).await?;
    assert!(held.record.settled_at.is_none());
    assert!(
        held.record
            .activity
            .as_ref()
            .is_some_and(|activity| activity.held)
    );
    let stopped = fixture
        .host
        .take()
        .ok_or("history prune Host absent")?
        .stop_for_readback()
        .await?;
    prune_history(&stopped, control, &held.record).await?;
    let retained = deliveries(handoffs(&stopped, &fixture.project, 1).await?)
        .into_iter()
        .find(|row| row.id() == item.id())
        .ok_or("original handoff absent after history prune")?;
    assert_eq!(
        serde_json::to_value(retained)?,
        json!({"kind":"item","value":serde_json::to_value(&item)?})
    );
    fixture.reopen_host().await?;
    let reopened = native_state(fixture.host()?, target, &item_id, PushDeliveryState::Held).await?;
    assert_eq!(reopened.record.body, held.record.body);
    assert_eq!(reopened.record.activity, held.record.activity);
    Ok(held.record)
}

async fn deliver_after_prune(
    fixture: &BoardFixture,
    peer: &AcpRecipient,
    control: &PushRecord,
    held: &PushRecord,
) -> FixtureResult {
    let text = peer.expect_text(FOLLOW_SUMMARY).await?;
    assert!(
        text.contains(held.push_id.as_str()),
        "retained follow changed its original actual item identity"
    );
    let shown = delivered(fixture.host()?, &peer.target, held.push_id.as_str()).await?;
    assert_eq!(shown.record.push_id, held.push_id);
    assert_eq!(
        shown
            .record
            .activity
            .as_ref()
            .map(|activity| &activity.ranges),
        held.activity.as_ref().map(|activity| &activity.ranges)
    );
    assert_eq!(peer.count_matching(FOLLOW_SUMMARY)?, 1);
    let unavailable = fixture
        .host()?
        .mcp_reply(
            "router_show",
            json!({"caller":peer.target,"reference":control.push_id}),
        )
        .await?;
    assert!(
        unavailable.is_error,
        "pruned terminal history was recreated during held delivery"
    );
    Ok(())
}

async fn terminal_control(
    fixture: &BoardFixture,
    peer: &AcpRecipient,
) -> FixtureResult<PushRecord> {
    let item = identity();
    let sent = fixture
        .host()?
        .mcp(
            "message_send",
            json!({"itemId":item,"target":peer.target,
        "message":{"kind":"humanUser","text":CONTROL_BODY},"delivery":"auto"}),
        )
        .await?;
    assert_eq!(
        sent.pointer("/receipt/outcome/kind"),
        Some(&json!("started"))
    );
    peer.expect_text(CONTROL_BODY).await?;
    let shown = fixture
        .host()?
        .mcp(
            "router_show",
            json!({"caller":peer.target,"reference":item}),
        )
        .await?;
    let shown: PushRecordShowResult = serde_json::from_value(shown)?;
    assert_eq!(shown.record.body.as_deref(), Some(CONTROL_BODY));
    assert!(shown.record.settled_at.is_some());
    Ok(shown.record)
}

async fn prune_history(
    stopped: &super::owned_host::StoppedHost,
    control: &PushRecord,
    held: &PushRecord,
) -> FixtureResult {
    let path = stopped.directory().join("interaction.sqlite");
    if !path.is_file() {
        return Err("real native history owner file absent".into());
    }
    let store = interaction_storage::InteractionStore::open(path).await?;
    run_scenario(
        store,
        async |store: &mut interaction_storage::InteractionStore| {
            let held_before = store
                .get_push_record(&held.push_id)
                .await?
                .ok_or("actual held push absent")?;
            assert_eq!(held_before.delivery_state, PushDeliveryState::Held);
            assert!(held_before.settled_at.is_none());
            let retention_months =
                chrono::Months::new(interaction_storage::INTERACTION_RETENTION_MONTHS);
            let future = held
                .created_at
                .checked_add_months(retention_months)
                .and_then(|future| future.checked_add_signed(chrono::Duration::days(1)))
                .ok_or("interaction retention test timestamp overflow")?;
            let cutoff = future
                .checked_sub_months(retention_months)
                .ok_or("interaction retention cutoff overflow")?;
            assert!(control.settled_at.is_some_and(|settled| settled < cutoff));
            let pruned = store.prune_push_records(future, 500).await?;
            assert!(
                pruned > 0,
                "real history pruning did not remove its terminal positive control"
            );
            assert!(store.get_push_record(&control.push_id).await?.is_none());
            let retained = store
                .get_push_record(&held.push_id)
                .await?
                .ok_or("held custody pruned with terminal history")?;
            assert_eq!(
                serde_json::to_value(retained)?,
                serde_json::to_value(&held_before)?
            );
            Ok(())
        },
        async |store: interaction_storage::InteractionStore| {
            store.close().await.map_err(Into::into)
        },
        || Ok(()),
    )
    .await
}
