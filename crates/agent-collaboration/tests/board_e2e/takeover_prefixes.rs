//! Three literal Copy prefixes decide which cause, allocation and generation survive.
use super::{
    central_pair::{SecondHost, settled_view, work_view},
    fixture::{BoardFixture, at},
    takeover_controls::{self as controls, BOUND},
    takeover_follows::{
        FollowMembership, TransferKind, delivered, only_item, submitted_item, transfer_to_second,
    },
    takeover_readbacks as readbacks,
    takeover_recipients::{self as recipients, Recipients},
};
use board_central_test_support::{DispatchCallSelector, ObjectAddress};
use board_sync_protocol::BoardDeliveryTypes;
use codex_router_test_support::FixtureResult;
use fleet_sync_protocol::DeliveryItem;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub enum ReplicatedPrefix {
    Neither,
    CauseOnly,
    Both,
}

pub async fn prefix(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    prefix: ReplicatedPrefix,
) -> FixtureResult {
    recipients::scenario(fixture, second, async move |fixture, second, peers| {
        Box::pin(controls::scenario(
            fixture,
            second,
            async move |fixture, second| prove_prefix(fixture, second, peers, prefix).await,
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
    let range = follow
        .post(fixture, "one literal takeover owed post")
        .await?;
    let expected = acknowledged_cut(fixture, &follow, &range, &baseline, prefix).await?;
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
    assert_copy_cut(fixture, &stopped, &range, prefix, original.as_ref()).await?;
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

async fn acknowledged_cut(
    fixture: &BoardFixture,
    follow: &FollowMembership,
    range: &Value,
    baseline: &Value,
    prefix: ReplicatedPrefix,
) -> FixtureResult<Option<DeliveryItem<BoardDeliveryTypes>>> {
    Ok(match prefix {
        ReplicatedPrefix::Neither => {
            controls::held(fixture, controls::connection(fixture)?).await?;
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

async fn assert_copy_cut(
    fixture: &BoardFixture,
    stopped: &super::owned_host::StoppedHost,
    range: &Value,
    prefix: ReplicatedPrefix,
    original: Option<&DeliveryItem<BoardDeliveryTypes>>,
) -> FixtureResult {
    let cursor = serde_json::from_value(at(range, "/through")?.clone())?;
    let cause = readbacks::cause_capture(stopped, &fixture.project, cursor).await?;
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
            controls::confirm_copy_ack(fixture, cause).await?,
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
    assert_eq!(copy.epoch, cause.epoch);
    match prefix {
        ReplicatedPrefix::Neither => assert!(copy.sequence < cause.sequence),
        ReplicatedPrefix::CauseOnly => {
            assert!(copy.sequence >= cause.sequence);
            assert!(
                copy.sequence
                    < handoff
                        .ok_or("cause-only has no local allocation")?
                        .sequence
            );
        }
        ReplicatedPrefix::Both => {
            assert!(copy.sequence >= handoff.ok_or("both has no allocation")?.sequence)
        }
    }
    eprintln!(
        "T native prefix evidence: {}",
        json!({"prefix":name,"cause":cause,"handoff":handoff,"copyAck":copy})
    );
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
