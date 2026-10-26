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
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem};
use serde_json::{Value, json};

struct HeldFollow {
    follow: FollowMembership,
    item: DeliveryItem<BoardDeliveryTypes>,
    range: Value,
}

async fn held_remote(fixture: &mut BoardFixture, second: &SecondHost) -> FixtureResult<HeldFollow> {
    fixture.create().await?;
    let follow =
        FollowMembership::arrange(fixture, recipients::session(second.host()?).await?).await?;
    settled_view(fixture.host()?, &fixture.project).await?;
    controls::hold_selected(
        fixture,
        ObjectAddress::Registry,
        ObjectAddress::Mailbox(second.service_id()?.try_into()?),
        DispatchCallSelector::MailboxSubmit,
    )
    .await?;
    let range = follow
        .post(fixture, "held follow cancellation range")
        .await?;
    let item = submitted_item(
        &controls::held_call(fixture, ObjectAddress::Registry, "central.mailbox-submit").await?,
    )?;
    follow.assert_frozen(&item, &range)?;
    settled_view(fixture.host()?, &fixture.project).await?;
    controls::resume(fixture, ObjectAddress::Registry).await?;
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
    prefix: EndPrefix,
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
    prefix: EndPrefix,
) -> FixtureResult {
    let seed = held_remote(fixture, second).await?;
    let mailbox = ObjectAddress::Mailbox(second.service_id()?.try_into()?);
    match prefix {
        EndPrefix::Discarded => {
            controls::hold_selected(
                fixture,
                controls::connection(fixture)?,
                controls::copy(fixture)?,
                DispatchCallSelector::CopyWrite,
            )
            .await?
        }
        EndPrefix::LostReceipt => {
            controls::hold_selected(
                fixture,
                ObjectAddress::Registry,
                mailbox.clone(),
                DispatchCallSelector::MailboxSubmit,
            )
            .await?
        }
        EndPrefix::Retained => {}
    }
    let pre_end_ack = controls::copy_ack(fixture).await?;
    let end = seed.follow.unsubscribe(fixture).await?;
    let mut held_copy_write = None;
    let lost = match prefix {
        EndPrefix::Discarded => {
            held_copy_write = Some(controls::held(fixture, controls::connection(fixture)?).await?);
            native_state(
                second.host()?,
                &seed.follow.target,
                &seed.item.id().as_uuid().to_string(),
                PushDeliveryState::Held,
            )
            .await?;
            None
        }
        EndPrefix::Retained => {
            sender_status(
                fixture.host()?,
                &seed.item.id().as_uuid().to_string(),
                "cancelled",
            )
            .await?;
            settled_view(fixture.host()?, &fixture.project).await?;
            None
        }
        EndPrefix::LostReceipt => {
            Some(lose_cancel_answer(fixture, mailbox, seed.item.id().as_uuid().to_string()).await?)
        }
    };
    let stopped = fixture
        .host
        .take()
        .ok_or("predecessor Host absent")?
        .crash_for_readback()
        .await?;
    let before = readbacks::handoffs(&stopped, &fixture.project, 1).await?;
    assert_end_groups(&before, &seed.follow, true)?;
    let expansions = readbacks::cancel_expansions(&stopped, &fixture.project, 1).await?;
    if !matches!(prefix, EndPrefix::Discarded) {
        let [expansion] = expansions.as_slice() else {
            return Err("retained end has no unique frozen child cancel".into());
        };
        assert_eq!(expansion.get("target"), Some(&json!(seed.item.id())));
        if let Some(lost) = lost {
            assert_eq!(expansion.get("envelope"), Some(&lost));
        }
    }
    controls::assert_end_cut(
        fixture,
        &stopped,
        seed.item.id(),
        end,
        prefix,
        &expansions,
        pre_end_ack,
    )
    .await?;
    if let Some(request) = held_copy_write {
        assert_eq!(
            request.get("kind"),
            Some(&json!("central.copy-session-write"))
        );
    }
    drop(stopped);
    recipient_before_transfer(second, &seed, prefix).await?;
    transfer_to_second(fixture, second, TransferKind::Takeover).await?;
    if matches!(prefix, EndPrefix::Discarded) {
        controls::resume(fixture, controls::connection(fixture)?).await?;
    }
    assert_remote_successor(fixture, second, peers, prefix, &seed, expansions).await
}

async fn assert_remote_successor(
    fixture: &BoardFixture,
    second: &mut SecondHost,
    peers: &mut Recipients,
    prefix: EndPrefix,
    seed: &HeldFollow,
    expansions: Vec<Value>,
) -> FixtureResult {
    peers
        .start_second(second, u64::from(matches!(prefix, EndPrefix::Discarded)))
        .await?;
    let expected_state = match prefix {
        EndPrefix::Discarded => {
            peers
                .second()?
                .expect_marker(&seed.item.id().as_uuid().to_string())
                .await?;
            PushDeliveryState::Delivered
        }
        EndPrefix::Retained | EndPrefix::LostReceipt => PushDeliveryState::Rejected,
    };
    native_state(
        second.host()?,
        &seed.follow.target,
        &seed.item.id().as_uuid().to_string(),
        expected_state,
    )
    .await?;
    if !matches!(prefix, EndPrefix::Discarded) {
        let [frozen] = expansions.as_slice() else {
            return Err("retained end has no unique frozen cancel association".into());
        };
        let cancel = serde_json::from_value(
            frozen
                .get("cancel")
                .ok_or("frozen child cancel is absent")?
                .clone(),
        )?;
        let host = second.host_mut()?;
        // Rejected already held before takeover; only this committed row proves replay.
        tokio::time::timeout(BOUND, async {
            while !host.local_cancel_delivery_recorded(cancel).await? {}
            FixtureResult::Ok(())
        })
        .await??;
    }
    let stopped = second.stop_for_readback().await?;
    let inherited = readbacks::handoffs(&stopped, &fixture.project, 2).await?;
    if matches!(prefix, EndPrefix::Discarded) {
        readbacks::assert_ongoing_membership(
            &stopped,
            &fixture.project,
            2,
            &seed.follow.membership,
        )
        .await?;
    }
    assert_end_groups(
        &inherited,
        &seed.follow,
        !matches!(prefix, EndPrefix::Discarded),
    )?;
    let actual = only_item(readbacks::deliveries(inherited))?;
    assert_eq!(
        serde_json::to_value(&actual)?,
        serde_json::to_value(&seed.item)?
    );
    seed.follow.assert_frozen(&actual, &seed.range)?;
    let after = readbacks::cancel_expansions(&stopped, &fixture.project, 2).await?;
    match prefix {
        EndPrefix::Discarded => assert!(after.is_empty()),
        EndPrefix::Retained | EndPrefix::LostReceipt => assert_eq!(after, expansions),
    }
    assert_local_replay(
        &readbacks::local_cancel_deliveries(&stopped).await?,
        prefix,
        &expansions,
    )?;
    readbacks::assert_no_wake(&stopped).await?;
    drop(stopped);
    second.reopen_host().await?;
    native_state(
        second.host()?,
        &seed.follow.target,
        &seed.item.id().as_uuid().to_string(),
        expected_state,
    )
    .await?;
    assert_eq!(
        peers.second()?.count_after_barrier().await?,
        u64::from(matches!(prefix, EndPrefix::Discarded))
    );
    Ok(())
}

async fn lose_cancel_answer(
    fixture: &BoardFixture,
    mailbox: ObjectAddress,
    original: String,
) -> FixtureResult<Value> {
    tokio::time::timeout(BOUND, async {
        loop {
            let request = controls::held(fixture, ObjectAddress::Registry).await?;
            if request.get("kind") == Some(&json!("central.mailbox-submit")) {
                let item: DeliveryEnvelope<BoardDeliveryTypes> =
                    serde_json::from_value(at(&request, "/request/item")?.clone())?;
                if let DeliveryEnvelope::Cancel(cancel) = item {
                    assert_eq!(cancel.target().as_uuid().to_string(), original);
                    let dropped = controls::release_and_drop_selected_reply(
                        fixture,
                        ObjectAddress::Registry,
                        mailbox.clone(),
                        DispatchCallSelector::MailboxSubmit,
                    )
                    .await?;
                    assert_eq!(dropped, request);
                    return Ok(at(&request, "/request/item")?.clone());
                }
            }
            super::central_pair::central(fixture)?
                .runtime_control(
                    ObjectAddress::Registry,
                    RuntimeControl::ResumeDispatchAndHoldNextSelected {
                        destination: mailbox.clone(),
                        call: DispatchCallSelector::MailboxSubmit,
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

fn assert_local_replay(local: &[Value], prefix: EndPrefix, expansions: &[Value]) -> FixtureResult {
    if matches!(prefix, EndPrefix::Discarded) {
        assert!(local.is_empty(), "discarded end invented a Local cancel");
        return Ok(());
    }
    let [replayed] = local else {
        return Err("successor did not admit exactly one Local cancel replay".into());
    };
    let [frozen] = expansions else {
        return Err("retained end has no unique frozen cancel association".into());
    };
    assert_eq!(replayed.get("cancel"), frozen.get("cancel"));
    assert_eq!(replayed.get("target"), frozen.get("target"));
    assert_eq!(
        replayed.get("attribution"),
        Some(&json!({"kind":"boardDerived"}))
    );
    Ok(())
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
    let mut wire = super::central_pair::central(fixture)?.observe_mail_wire(None)?;
    let mut status_wire = second.enrolled_machine()?.observe_mail_wire(None)?;
    // Arrange the existing test-only connected-status cadence before any transfer.
    second.stop_host().await?;
    std::fs::write(
        second.directory.join("fixture-mail-status-interval-ms"),
        "2000",
    )?;
    second.reopen_host().await?;
    let HeldFollow { follow, item, .. } = held_local_follow(fixture).await?;
    controls::hold(
        fixture,
        controls::connection(fixture)?,
        controls::copy(fixture)?,
    )
    .await?;
    let pre_end_ack = controls::copy_ack(fixture).await?;
    let end = follow.unsubscribe(fixture).await?;
    controls::held(fixture, controls::connection(fixture)?).await?;
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
    controls::resume(fixture, controls::connection(fixture)?).await?;
    fixture.reopen_host().await?;
    peers.start_first(fixture, 0).await?;
    sender_status(
        second.host()?,
        &item.id().as_uuid().to_string(),
        "atMachine",
    )
    .await?;
    let cancel = assert_local_prevention(
        fixture,
        &follow,
        &item,
        &rejected,
        &mut wire,
        &mut status_wire,
    )
    .await?;
    sender_status(
        second.host()?,
        &item.id().as_uuid().to_string(),
        "atMachine",
    )
    .await?;
    let stopped = second.stop_for_readback().await?;
    let inherited = readbacks::handoffs(&stopped, &fixture.project, 2).await?;
    readbacks::assert_ongoing_membership(&stopped, &fixture.project, 2, &follow.membership).await?;
    assert_end_groups(&inherited, &follow, false)?;
    let actual = only_item(readbacks::deliveries(inherited))?;
    assert_eq!(serde_json::to_value(actual)?, serde_json::to_value(&item)?);
    readbacks::assert_no_wake(&stopped).await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("recipient Host absent")?
        .stop_for_readback()
        .await?;
    readbacks::assert_no_wake(&stopped).await?;
    readbacks::assert_prevented_report_stopped(&stopped, item.id(), cancel).await?;
    assert_eq!(peers.first()?.count_after_barrier().await?, 0);
    assert_eq!(controls::report_count(&wire, item.id()), 1);
    eprintln!(
        "T held-local containment evidence: {}",
        json!({"item":item.id(),"reportSeq":3,"cancel":cancel,"typedTerminalRefusal":true,"reportSends":1,"minimumFutureStatusScans":6,"recipientReopened":true,"nativeFinal":"prevented","runningReport":false,"senderStatus":"atMachine","namedLimit":"SDK0.3.1 sender/central terminal projection"})
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
) -> FixtureResult<fleet_sync_protocol::ItemId> {
    let cancel = controls::assert_prevented_report_refused(wire, item.id()).await?;
    controls::assert_report_quiet(wire, status_wire, item.id())
        .await
        .map_err(|error| format!("before recipient reopen: {error}"))?;
    let stopped = fixture
        .host
        .take()
        .ok_or("recipient Host absent")?
        .stop_for_readback()
        .await?;
    readbacks::assert_prevented_report_stopped(&stopped, item.id(), cancel).await?;
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
    controls::assert_report_quiet(wire, status_wire, item.id())
        .await
        .map_err(|error| format!("after recipient reopen: {error}"))?;
    Ok(cancel)
}

async fn held_local_follow(fixture: &mut BoardFixture) -> FixtureResult<HeldFollow> {
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
    Ok(HeldFollow {
        follow,
        item,
        range,
    })
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
