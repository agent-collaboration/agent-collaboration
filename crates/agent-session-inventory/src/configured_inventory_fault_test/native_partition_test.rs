use super::*;
use crate::picker_runtime_status::{NativeLiveReadState, NativeLiveUnavailableReason};
use crate::test_support::source_fault_fixture::native_partitions::PRIVATE_SENTINEL;
use crate::test_support::source_fault_fixture::{
    ReadPeer, ReplyScenario, native_partitions::NativeScenario,
};
#[tokio::test]
async fn configured_inventory_native_partitions_preserve_prefix_empty_and_qualified_origins() {
    for (scenario, expected_rows, expected_state) in [
        (
            NativeScenario::Unavailable,
            1,
            NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable),
        ),
        (NativeScenario::Empty, 1, NativeLiveReadState::Complete),
        (
            NativeScenario::PrefixUnavailable,
            2,
            NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable),
        ),
        (
            NativeScenario::ChannelLess,
            1,
            NativeLiveReadState::Complete,
        ),
    ] {
        let peer = ReadPeer::start(ReplyScenario::NativePartition(scenario)).await;
        let snapshot = crate::test_support::complete_source_reader::read_complete_source(
            peer.loader(),
            peer.request(),
        )
        .await
        .into_snapshot()
        .unwrap();
        assert_eq!(snapshot.records.len(), expected_rows, "{scenario:?}");
        assert_eq!(
            snapshot
                .native_live_coverage
                .iter()
                .find(|part| part.view == crate::picker_runtime_status::NativeCatalogueView::Loaded)
                .unwrap()
                .state,
            expected_state,
            "{scenario:?}"
        );
        assert!(!format!("{snapshot:?}").contains(PRIVATE_SENTINEL));
        if expected_rows == 2 {
            assert!(snapshot.records.iter().any(|row| matches!(
                row.identity,
                SessionPickerIdentity::HostedProvider(_)
            ) && row.cwd.as_deref()
                == Some("/source/managed")));
            assert!(snapshot.records.iter().any(|row| matches!(
                row.identity,
                SessionPickerIdentity::NativeClaude(_)
            ) && row.cwd.as_deref()
                == Some("/source/native")));
        }
        if matches!(scenario, NativeScenario::ChannelLess) {
            assert_eq!(
                snapshot.records.first().unwrap().client_provider_family(),
                crate::session_provider_family::SessionProviderFamily::External(
                    collaboration_client::protocol::ProviderKind::ClaudeCode
                ),
                "typed native origin must stay in Claude family when ACP is absent"
            );
            let calls = peer.state.calls.lock().unwrap();
            assert!(
                !calls
                    .iter()
                    .any(|(name, args)| name == "provider_sessions_list"
                        && args.get("view").and_then(Value::as_str) == Some("stored")
                        && args.get("source").and_then(Value::as_str) == Some("all")),
                "native-only membership must not require managed store"
            );
        }
        peer.finish().await;
    }
}

#[tokio::test]
async fn configured_inventory_native_invalid_reply_never_preserves_a_partial_success() {
    for (scenario, reason) in [
        (
            NativeScenario::InvalidParams,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::UnknownFailure,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::MalformedFailure,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::WrongErrorId,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::WrongOrigin,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::Duplicate,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::ChangedEpoch,
            SourceInventoryRejection::StaleSnapshot,
        ),
        (
            NativeScenario::WrongService,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::DuplicateEndpoint,
            SourceInventoryRejection::InvalidInventory,
        ),
    ] {
        let peer = ReadPeer::start(ReplyScenario::NativePartition(scenario)).await;
        let rejection = crate::test_support::complete_source_reader::read_complete_source(
            peer.loader(),
            peer.request(),
        )
        .await
        .into_snapshot()
        .unwrap_err();
        assert_eq!(rejection, reason, "{scenario:?}");
        assert!(!rejection.to_string().contains(PRIVATE_SENTINEL));
        peer.finish().await;
    }
    let peer = ReadPeer::start(ReplyScenario::NativePartition(
        NativeScenario::MissingEndpoint,
    ))
    .await;
    let snapshot = crate::test_support::complete_source_reader::read_complete_source(
        peer.loader(),
        peer.request(),
    )
    .await
    .into_snapshot()
    .unwrap();
    assert!(snapshot.records.is_empty() && snapshot.native_live_coverage.is_empty());
    assert_eq!(
        peer.page_calls(),
        1,
        "only the actual Codex Stored partition exists"
    );
    peer.finish().await;
}

#[tokio::test]
async fn configured_inventory_native_timeout_keeps_completed_stored_and_live_prefix() {
    let peer = ReadPeer::start(ReplyScenario::NativePartition(NativeScenario::PrefixHeld)).await;
    let loader = peer.loader();
    let request = peer.request();
    let read = tokio::spawn(async move {
        crate::test_support::complete_source_reader::read_complete_source(loader, request).await
    });
    peer.wait_for_held_page().await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    let snapshot = read
        .await
        .unwrap()
        .into_snapshot()
        .expect("extra live timeout must not erase either validated partition");
    tokio::time::resume();
    assert_eq!(snapshot.records.len(), 2);
    assert_eq!(
        snapshot
            .native_live_coverage
            .iter()
            .find(|part| part.view == crate::picker_runtime_status::NativeCatalogueView::Loaded)
            .unwrap()
            .state,
        NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::TimedOut)
    );
    assert!(
        snapshot
            .records
            .iter()
            .any(|row| matches!(row.identity, SessionPickerIdentity::HostedProvider(_)))
    );
    assert!(
        snapshot
            .records
            .iter()
            .any(|row| matches!(row.identity, SessionPickerIdentity::NativeClaude(_)))
    );
    peer.release_held_page().await;
    peer.finish().await;
}
