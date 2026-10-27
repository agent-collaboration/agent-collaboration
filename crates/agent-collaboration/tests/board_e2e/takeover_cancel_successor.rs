//! Match successor cancellation effects to the explicitly selected seed frontier.
use super::super::{
    central_pair::SecondHost,
    fixture::BoardFixture,
    takeover_controls::{BOUND, EndPrefix},
    takeover_follows::{native_state, only_item},
    takeover_readbacks as readbacks,
    takeover_recipients::Recipients,
};
use super::{FrontierCut, HeldFollow, RemoteEndPrefix, assert_end_groups};
use codex_router_test_support::FixtureResult;
use collaboration_protocol::PushDeliveryState;
use serde_json::{Value, json};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SeededHandoffFrontier {
    seeded_through: message_board::HandoffSeq,
    head: message_board::HandoffSeq,
    live_through: message_board::HandoffSeq,
}

pub(super) async fn assert_remote_successor(
    fixture: &BoardFixture,
    second: &mut SecondHost,
    peers: &mut Recipients,
    remote_prefix: RemoteEndPrefix,
    seed: &HeldFollow,
    expansions: Vec<Value>,
) -> FixtureResult {
    let prefix = remote_prefix.end_prefix();
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
    if let RemoteEndPrefix::Retained(cut) = remote_prefix {
        let observed = tokio::time::timeout(
            BOUND,
            second.host_mut()?.seeded_handoff_frontier(&fixture.project),
        )
        .await
        .map_err(|error| format!("CI6 successor verified seed frontier: {error}"))??;
        let inherited: SeededHandoffFrontier = serde_json::from_value(observed)?;
        assert_eq!(
            inherited.head.get(),
            3,
            "retained-end seed lost its handoff head"
        );
        assert!(
            inherited.live_through >= inherited.seeded_through,
            "successor frontier regressed below its seed"
        );
        match cut {
            FrontierCut::Unsettled => assert!(
                inherited.seeded_through.get() < 2,
                "owed-replay case inherited a floor covering the original"
            ),
            FrontierCut::Settled => assert_eq!(
                inherited.seeded_through, inherited.head,
                "settled case inherited an unsettled seed"
            ),
        }
        eprintln!("CI6 inherited frontier: {cut:?}, {inherited:?}");
    }
    wait_for_replay_or_publication(second, &fixture.project, remote_prefix, &expansions).await?;
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
        remote_prefix,
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

async fn wait_for_replay_or_publication(
    second: &mut SecondHost,
    project: &str,
    remote_prefix: RemoteEndPrefix,
    expansions: &[Value],
) -> FixtureResult {
    if remote_prefix.requires_local_replay() {
        let [frozen] = expansions else {
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
        .await
        .map_err(|error| {
            format!("CI6 successor Local cancel replay after unsettled seed: {error}")
        })??;
    }
    if matches!(
        remote_prefix,
        RemoteEndPrefix::Retained(FrontierCut::Settled)
    ) {
        // Exercise a complete real publisher pass before proving absence. This
        // barrier is not used to supply the owed replay in the unsettled case.
        tokio::time::timeout(BOUND, second.host_mut()?.drain_project_publisher(project))
            .await
            .map_err(|error| format!("CI6 settled-seed publisher completion: {error}"))??;
    }
    Ok(())
}

fn assert_local_replay(
    local: &[Value],
    prefix: RemoteEndPrefix,
    expansions: &[Value],
) -> FixtureResult {
    if !prefix.requires_local_replay() {
        assert!(
            local.is_empty(),
            "discarded or settled seed invented a Local cancel"
        );
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
