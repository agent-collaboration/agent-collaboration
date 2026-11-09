//! Two real Host compositions share the same persisted native session identity.
use super::*;
use board_sync_protocol::{FleetLinkRecord, FleetLinkRefusal, FleetLinkState};

#[tokio::test]
async fn two_hosts_with_the_same_session_keep_the_latest_connected_without_flip_flop() {
    // Arrange: one actual central and Host, then copy only its stable identity/resume data.
    let first = start_host(true).await;
    let central = first._central.as_ref().expect("actual central");
    await_link(&first.runtime, |state| {
        matches!(state, FleetLinkState::Connected { .. })
    })
    .await;
    let cache_store = message_board_storage::FleetStore::open(
        &first
            .directory
            .path()
            .join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await
    .unwrap();
    let cache = cache_store.cache();
    let resume = cache
        .machine_session_state()
        .await
        .unwrap()
        .expect("first native resume state");
    cache_store.close().await;
    let second_directory = tempfile::Builder::new()
        .prefix("same-session-host-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(
        second_directory.path(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    std::fs::create_dir(second_directory.path().join("peers")).unwrap();
    std::fs::set_permissions(
        second_directory.path().join("peers"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    for identity in ["installation-peer.json", "service-identity.json"] {
        std::fs::copy(
            first.directory.path().join(identity),
            second_directory.path().join(identity),
        )
        .unwrap();
    }
    let cache_store = message_board_storage::FleetStore::open(
        &second_directory
            .path()
            .join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await
    .unwrap();
    let cache = cache_store.cache();
    cache.save_machine_session_state(&resume).await.unwrap();
    cache_store.close().await;

    // Act: the second real Host attaches the same peer AND session id.
    let second = StartupFixture {
        runtime: start_runtime(
            second_directory.path(),
            Some(central.session_inputs().unwrap()),
        )
        .await,
        directory: second_directory,
        _central: None,
    };
    let winner = await_link(&second.runtime, |state| {
        matches!(state, FleetLinkState::Connected { .. })
    })
    .await;
    tokio::time::timeout(
        BOUND,
        first
            .runtime
            .board_session
            .as_ref()
            .expect("first session owner")
            .wait_for_stop_for_test(),
    )
    .await
    .expect("replaced Host never stopped; duplicate session keeps reconnecting")
    .expect("stop observation");

    // Assert: the actual session stop fact precedes readback; no absence-by-sleep oracle.
    let loser = await_link(&first.runtime, |state| {
        matches!(
            state,
            FleetLinkState::Refused {
                reason: FleetLinkRefusal::IdentityInUseElsewhere,
                ..
            }
        )
    })
    .await;
    assert_eq!(loser.peer, winner.peer);
    assert_eq!(
        central.session_connections(),
        2,
        "one of the Hosts took the peer back"
    );
    let cache_store = message_board_storage::FleetStore::open(
        &second
            .directory
            .path()
            .join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await
    .unwrap();
    let cache = cache_store.cache();
    assert_eq!(
        cache
            .machine_session_state()
            .await
            .unwrap()
            .unwrap()
            .session_id,
        resume.session_id
    );
    cache_store.close().await;
    let old_identity = client(&first).await.installation_show().await.unwrap();
    let new_identity = client(&second).await.installation_show().await.unwrap();
    assert_eq!(old_identity.peer_id, new_identity.peer_id);
    first
        .runtime
        .shutdown()
        .await
        .expect("known refusal must not fail Host cleanup");
    let retained = second.runtime.fleet_link.read().await.unwrap().unwrap();
    assert!(matches!(retained.state, FleetLinkState::Connected { .. }));
    assert_eq!(
        central.session_connections(),
        2,
        "cleanup or replacement reconnected the old Host"
    );
    second.runtime.shutdown().await.expect("winner cleanup");
}

async fn await_link(
    runtime: &CollaborationRuntime,
    expected: impl Fn(&FleetLinkState) -> bool,
) -> FleetLinkRecord {
    tokio::time::timeout(BOUND, async {
        loop {
            let record = runtime
                .fleet_link
                .read()
                .await
                .unwrap()
                .expect("configured link");
            if expected(&record.state) {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Host did not publish expected link state")
}
