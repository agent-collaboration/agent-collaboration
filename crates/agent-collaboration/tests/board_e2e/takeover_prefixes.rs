//! Three literal Copy prefixes decide which cause, allocation and generation survive.
use super::{
    central_pair::{SecondHost, settled_view, work_view},
    fixture::{BoardFixture, accepted, at, identity},
    takeover_controls::{self as controls, BOUND},
    takeover_follows::{
        FollowMembership, TransferKind, delivered, only_item, submitted_item, transfer_to_second,
    },
    takeover_readbacks as readbacks,
    takeover_recipients::{self as recipients, Recipients},
};
use board_central_test_support::{DispatchCallSelector, ObjectAddress};
use board_sync_protocol::{BoardAnnotation, BoardDeliveryTypes};
use codex_router_test_support::FixtureResult;
use fleet_sync_protocol::{DeliveryItem, ReplicationPosition, ReplicationRequest};
use fleet_sync_writer::CapturedTransaction;
use message_board_storage::CapturedProjectChanges;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub enum ReplicatedPrefix {
    Neither,
    CauseOnly,
    Both,
}

#[derive(Clone, Copy)]
enum LocalCaptureCut {
    Observe,
    HoldPriorPiece,
}

pub async fn prefix(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    prefix: ReplicatedPrefix,
) -> FixtureResult {
    prefix_at_capture_cut(fixture, second, prefix, LocalCaptureCut::Observe).await
}

pub async fn uncaptured_prefix(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
) -> FixtureResult {
    prefix_at_capture_cut(
        fixture,
        second,
        ReplicatedPrefix::Neither,
        LocalCaptureCut::HoldPriorPiece,
    )
    .await
}

async fn prefix_at_capture_cut(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    prefix: ReplicatedPrefix,
    capture_cut: LocalCaptureCut,
) -> FixtureResult {
    recipients::scenario(fixture, second, async move |fixture, second, peers| {
        Box::pin(controls::scenario(
            fixture,
            second,
            async move |fixture, second| {
                prove_prefix(fixture, second, peers, prefix, capture_cut).await
            },
        ))
        .await
    })
    .await
}

async fn prove_prefix(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    peers: &mut Recipients,
    prefix: ReplicatedPrefix,
    capture_cut: LocalCaptureCut,
) -> FixtureResult {
    fixture.create().await?;
    peers
        .start_second(
            second,
            u64::from(!matches!(prefix, ReplicatedPrefix::Neither)),
        )
        .await?;
    let follow =
        FollowMembership::arrange(fixture, recipients::session(second.host()?).await?).await?;
    let baseline = settled_view(fixture.host()?, &fixture.project).await?;
    match prefix {
        ReplicatedPrefix::Neither => {
            controls::hold(
                fixture,
                controls::connection(fixture)?,
                controls::copy(fixture)?,
            )
            .await?
        }
        ReplicatedPrefix::CauseOnly => {
            controls::hold_selected(
                fixture,
                controls::connection(fixture)?,
                ObjectAddress::Registry,
                DispatchCallSelector::InstallationPeer,
            )
            .await?
        }
        ReplicatedPrefix::Both => {
            controls::hold_selected(
                fixture,
                ObjectAddress::Registry,
                ObjectAddress::Mailbox(second.service_id()?.try_into()?),
                DispatchCallSelector::MailboxSubmit,
            )
            .await?
        }
    }
    if matches!(capture_cut, LocalCaptureCut::HoldPriorPiece) {
        // Hold an unrelated real Copy before committing the followed post.
        // The actual stopped-owner readback below must then prove no capture.
        commit_and_hold_prior_piece(fixture).await?;
    }
    // In the prior-piece cut, the hold blocks further same-copy ACK progress.
    // Sample after that barrier and before the post; other cuts use only its epoch.
    let pre_post_ack = controls::copy_ack(fixture).await?;
    let range = follow
        .post(fixture, "one literal takeover owed post")
        .await?;
    let expected =
        acknowledged_cut(fixture, &follow, &range, &baseline, prefix, capture_cut).await?;
    let copy_before_crash = controls::copy_ack(fixture).await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("owning Host absent")?
        .crash_for_readback()
        .await?;
    let old = readbacks::deliveries(readbacks::handoffs(&stopped, &fixture.project, 1).await?);
    let original = match prefix {
        ReplicatedPrefix::Neither => old.into_iter().next().and_then(|envelope| match envelope {
            fleet_sync_protocol::DeliveryEnvelope::Item(item) => Some(item),
            fleet_sync_protocol::DeliveryEnvelope::Cancel(_) => None,
        }),
        ReplicatedPrefix::CauseOnly | ReplicatedPrefix::Both => {
            let item = only_item(old)?;
            follow.assert_frozen(&item, &range)?;
            if let Some(expected) = expected {
                assert_eq!(
                    serde_json::to_value(&item)?,
                    serde_json::to_value(expected)?
                );
            }
            Some(item)
        }
    };
    assert_copy_cut(
        fixture,
        &stopped,
        &range,
        prefix,
        original.as_ref(),
        pre_post_ack,
        capture_cut,
    )
    .await?;
    eprintln!("T Copy ACK before crash: {copy_before_crash:?}");
    drop(stopped);
    transfer_to_second(fixture, second, TransferKind::Takeover).await?;
    match prefix {
        ReplicatedPrefix::Neither | ReplicatedPrefix::CauseOnly => {
            controls::resume(fixture, controls::connection(fixture)?).await?;
        }
        ReplicatedPrefix::Both => {}
    }
    assert_successor(fixture, second, peers, &follow, &range, prefix, original).await
}

async fn commit_and_hold_prior_piece(fixture: &BoardFixture) -> FixtureResult {
    let marker = identity();
    let request = identity();
    let actor = fixture.owner.to_string();
    let committed = fixture
        .cli_write(&[
            "workstream",
            "create",
            "--project-id",
            &fixture.project,
            "--workstream-id",
            &marker,
            "--name",
            "earlier held Copy",
            "--goal",
            "hold a preceding write before the followed post",
            "--request-id",
            &request,
            "--actor",
            &actor,
        ])
        .await?;
    accepted(&committed)?;
    controls::held(fixture, controls::connection(fixture)?).await?;
    Ok(())
}

async fn acknowledged_cut(
    fixture: &BoardFixture,
    follow: &FollowMembership,
    range: &Value,
    baseline: &Value,
    prefix: ReplicatedPrefix,
    capture_cut: LocalCaptureCut,
) -> FixtureResult<Option<DeliveryItem<BoardDeliveryTypes>>> {
    Ok(match prefix {
        ReplicatedPrefix::Neither => {
            match capture_cut {
                LocalCaptureCut::Observe => hold_post_capture(fixture, range).await?,
                LocalCaptureCut::HoldPriorPiece => {
                    controls::held(fixture, controls::connection(fixture)?).await?;
                }
            }
            let observed = work_view(fixture.host()?, &fixture.project).await?;
            assert_eq!(
                observed.get("replicatedThrough"),
                baseline.get("replicatedThrough")
            );
            None
        }
        ReplicatedPrefix::CauseOnly => {
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
            let cause_prefix = settled_view(fixture.host()?, &fixture.project).await?;
            controls::confirm_prefix(fixture, fixture.host()?, at(range, "/through")?).await?;
            controls::advance_hold(
                fixture,
                controls::connection(fixture)?,
                controls::copy(fixture)?,
            )
            .await?;
            controls::held(fixture, controls::connection(fixture)?).await?;
            let held = work_view(fixture.host()?, &fixture.project).await?;
            assert_eq!(
                held.get("replicatedThrough"),
                cause_prefix.get("replicatedThrough")
            );
            None
        }
        ReplicatedPrefix::Both => {
            let submitted =
                controls::held_call(fixture, ObjectAddress::Registry, "central.mailbox-submit")
                    .await?;
            let original = submitted_item(&submitted)?;
            follow.assert_frozen(&original, range)?;
            let prefix = settled_view(fixture.host()?, &fixture.project).await?;
            controls::confirm_prefix(fixture, fixture.host()?, at(&prefix, "/asOf")?).await?;
            Some(original)
        }
    })
}

async fn hold_post_capture(fixture: &BoardFixture, range: &Value) -> FixtureResult {
    let cursor: message_board::FeedCursor = serde_json::from_value(at(range, "/through")?.clone())?;
    tokio::time::timeout(BOUND, async {
        loop {
            let held = controls::held(fixture, controls::connection(fixture)?).await?;
            let request: ReplicationRequest<BoardAnnotation> =
                serde_json::from_value(at(&held, "/request/request")?.clone())?;
            let ReplicationRequest::Piece(piece) = request else {
                return Err("post capture gate requires a real Copy Piece".into());
            };
            for transaction in &piece.transactions {
                let changes = CapturedProjectChanges::decode(&CapturedTransaction {
                    position: ReplicationPosition {
                        epoch: piece.epoch,
                        sequence: transaction.commit_id(),
                    },
                    transaction,
                })?;
                if changes
                    .first_feed_position
                    .is_some_and(|first| first <= cursor.position)
                    && changes.latest_feed.is_some_and(|latest| {
                        latest.cursor.epoch == cursor.epoch
                            && latest.cursor.position >= cursor.position
                    })
                {
                    // The writer commits capture_feed_marks before transport.
                    // Keep this Piece held; assert_copy_cut reads the real mark
                    // and unchanged Copy ACK after the owning Host is joined.
                    return Ok(());
                }
            }
            // A settled feed can still leave an auxiliary WAL capture in flight.
            // Release only that earlier Piece, atomically holding the next one.
            controls::advance_hold(
                fixture,
                controls::connection(fixture)?,
                controls::copy(fixture)?,
            )
            .await?;
        }
    })
    .await?
}

async fn assert_copy_cut(
    fixture: &BoardFixture,
    stopped: &super::owned_host::StoppedHost,
    range: &Value,
    prefix: ReplicatedPrefix,
    original: Option<&DeliveryItem<BoardDeliveryTypes>>,
    pre_post_ack: ReplicationPosition,
    capture_cut: LocalCaptureCut,
) -> FixtureResult {
    let cursor = serde_json::from_value(at(range, "/through")?.clone())?;
    readbacks::assert_local_post_at_cursor(stopped, &fixture.project, cursor).await?;
    // A held earlier settlement can precede capture of the new committed post.
    // Neither still means that central acknowledged neither cause nor handoff.
    let cause = match (prefix, capture_cut) {
        (ReplicatedPrefix::Neither, LocalCaptureCut::HoldPriorPiece) => {
            let cause =
                readbacks::optional_cause_capture(stopped, &fixture.project, cursor).await?;
            assert!(
                cause.is_none(),
                "the prior-piece control must force the uncaptured native cut"
            );
            cause
        }
        _ => Some(readbacks::cause_capture(stopped, &fixture.project, cursor).await?),
    };
    let handoff = match (prefix, original) {
        (ReplicatedPrefix::Neither, _) => None,
        (_, Some(item)) => {
            Some(readbacks::handoff_capture(stopped, &fixture.project, item.id()).await?)
        }
        (_, None) => return Err("retained prefix has no frozen handoff".into()),
    };
    let (name, copy) = match prefix {
        ReplicatedPrefix::Neither => ("neither", controls::copy_ack(fixture).await?),
        ReplicatedPrefix::CauseOnly => (
            "causeOnly",
            controls::confirm_copy_ack(fixture, cause.ok_or("retained cause has no capture")?)
                .await?,
        ),
        ReplicatedPrefix::Both => (
            "both",
            controls::confirm_copy_ack(
                fixture,
                handoff.ok_or("both prefix has no frozen handoff capture")?,
            )
            .await?,
        ),
    };
    assert_eq!(copy.epoch.value(), cursor.epoch.get());
    assert_eq!(copy.epoch, pre_post_ack.epoch);
    if let Some(cause) = cause {
        assert_eq!(copy.epoch, cause.epoch);
    }
    match (prefix, cause) {
        (ReplicatedPrefix::Neither, Some(cause)) => {
            assert!(discarded_copy_excludes_cause(Some(cause), copy));
        }
        (ReplicatedPrefix::Neither, None) => assert_eq!(copy, pre_post_ack),
        (ReplicatedPrefix::CauseOnly, Some(cause)) => {
            assert!(copy.sequence >= cause.sequence);
            assert!(
                copy.sequence
                    < handoff
                        .ok_or("cause-only has no local allocation")?
                        .sequence
            );
        }
        (ReplicatedPrefix::Both, Some(_)) => {
            assert!(copy.sequence >= handoff.ok_or("both has no allocation")?.sequence)
        }
        (_, None) => return Err("retained prefix requires an actual native capture".into()),
    }
    eprintln!(
        "T native prefix evidence: {}",
        json!({"prefix":name,"cause":cause,"handoff":handoff,"prePostAck":pre_post_ack,"copyAck":copy})
    );
    Ok(())
}

fn discarded_copy_excludes_cause(
    cause: Option<ReplicationPosition>,
    copy: ReplicationPosition,
) -> bool {
    cause.is_some_and(|cause| copy.epoch == cause.epoch && copy.sequence < cause.sequence)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn discarded_copy_rejects_a_missing_cause_capture() -> FixtureResult {
    let ack_before_crash = serde_json::from_value(json!({"epoch":1,"sequence":99}))?;
    let advanced_ack = serde_json::from_value(json!({"epoch":1,"sequence":100}))?;
    let regressed_ack = serde_json::from_value(json!({"epoch":1,"sequence":98}))?;
    let ack_in_other_epoch = serde_json::from_value(json!({"epoch":2,"sequence":99}))?;
    assert!(!discarded_copy_excludes_cause(None, ack_before_crash));
    assert!(!discarded_copy_excludes_cause(None, advanced_ack));
    assert!(!discarded_copy_excludes_cause(None, regressed_ack));
    assert!(!discarded_copy_excludes_cause(None, ack_in_other_epoch));
    Ok(())
}

async fn assert_successor(
    fixture: &BoardFixture,
    second: &mut SecondHost,
    peers: &mut Recipients,
    follow: &FollowMembership,
    range: &Value,
    prefix: ReplicatedPrefix,
    original: Option<DeliveryItem<BoardDeliveryTypes>>,
) -> FixtureResult {
    let recipient_line = match prefix {
        ReplicatedPrefix::Neither => None,
        ReplicatedPrefix::CauseOnly | ReplicatedPrefix::Both => Some(
            peers
                .second()?
                .expect_text_with_timeout("router://", BOUND)
                .await?,
        ),
    };
    // A stopped owning snapshot is also a barrier after the successor's real
    // receive/replay path; no external reader races an owning writer.
    let stopped = second.stop_for_readback().await?;
    let actual = readbacks::deliveries(readbacks::handoffs(&stopped, &fixture.project, 2).await?);
    readbacks::assert_no_wake(&stopped).await?;
    if matches!(prefix, ReplicatedPrefix::Neither) {
        let cursor = serde_json::from_value(at(range, "/through")?.clone())?;
        assert!(
            !readbacks::contains_post_at(&stopped, &fixture.project, 2, cursor).await?,
            "discarded source post reached the successor"
        );
    }
    drop(stopped);
    match prefix {
        ReplicatedPrefix::Neither => {
            assert!(actual.is_empty(), "discarded cause invented a frozen item")
        }
        ReplicatedPrefix::CauseOnly | ReplicatedPrefix::Both => {
            let item = only_item(actual)?;
            follow.assert_frozen(&item, range)?;
            assert!(
                recipient_line
                    .as_ref()
                    .ok_or("recipient frame absent")?
                    .contains(&item.id().as_uuid().to_string())
            );
            let old = original.ok_or("predecessor frozen item missing")?;
            match prefix {
                ReplicatedPrefix::Both => {
                    assert_eq!(serde_json::to_value(&item)?, serde_json::to_value(&old)?)
                }
                ReplicatedPrefix::CauseOnly => assert_ne!(item.id(), old.id()),
                ReplicatedPrefix::Neither => {
                    return Err("lost prefix reached retained allocation".into());
                }
            }
            second.reopen_host().await?;
            let shown = delivered(
                second.host()?,
                &follow.target,
                &item.id().as_uuid().to_string(),
            )
            .await?;
            assert_eq!(
                shown.record.push_id.as_str(),
                item.id().as_uuid().to_string()
            );
            // Restoring the same owned store exercises startup replay under the
            // original identity, rather than trusting a sender receipt.
            settled_view(second.host()?, &fixture.project).await?;
            second.stop_host().await?;
            if matches!(prefix, ReplicatedPrefix::Both) {
                // The successor's effect precedes release of the old call.
                // Its dead caller need not receive a successful answer. The
                // outer peer owner checks the count after central fully joins.
                let stale =
                    controls::held_call(fixture, ObjectAddress::Registry, "central.mailbox-submit")
                        .await?;
                assert_eq!(
                    serde_json::to_value(submitted_item(&stale)?)?,
                    serde_json::to_value(&item)?
                );
                controls::resume(fixture, ObjectAddress::Registry).await?;
                second.reopen_host().await?;
                delivered(
                    second.host()?,
                    &follow.target,
                    &item.id().as_uuid().to_string(),
                )
                .await?;
                second.stop_host().await?;
            }
        }
    }
    let expected = u64::from(!matches!(prefix, ReplicatedPrefix::Neither));
    assert_eq!(
        tokio::time::timeout(BOUND, peers.second()?.count_after_barrier()).await??,
        expected
    );
    Ok(())
}
