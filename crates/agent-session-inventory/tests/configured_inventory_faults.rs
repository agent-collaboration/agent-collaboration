use super::*;
use crate::source_inventory_request::{
    PickerSourceContext, SourceInventoryRejection, SourceInventoryRequest,
};
use crate::test_support::source_fault_fixture::SERVICE;
use crate::test_support::source_fault_fixture::*;
use serde_json::Value;
use std::time::Duration;
#[tokio::test]
async fn protocol_faults_and_epoch_change_reject_without_empty_success_or_replay() {
    for (scenario, expected, native_calls) in [
        (
            ReplyScenario::ChangedEpoch,
            SourceInventoryRejection::StaleSnapshot,
            1,
        ),
        (
            ReplyScenario::WrongRequestId,
            SourceInventoryRejection::InvalidInventory,
            0,
        ),
        (
            ReplyScenario::WrongPageEndpoint,
            SourceInventoryRejection::InvalidInventory,
            1,
        ),
        (
            ReplyScenario::RepeatedCursor,
            SourceInventoryRejection::InvalidInventory,
            2,
        ),
        (
            ReplyScenario::PageBudget { complete: false },
            SourceInventoryRejection::InvalidInventory,
            65,
        ),
        (
            ReplyScenario::RowBudget(4097),
            SourceInventoryRejection::InvalidInventory,
            41,
        ),
        (
            ReplyScenario::ProviderSkipped,
            SourceInventoryRejection::InvalidInventory,
            0,
        ),
    ] {
        let peer = ReadPeer::start(scenario).await;
        let request = peer.request();
        let result = crate::test_support::complete_source_reader::read_complete_source(
            peer.loader(),
            request.clone(),
        )
        .await;
        assert_eq!(result.request(), &request);
        assert_eq!(result.into_snapshot().unwrap_err(), expected);
        assert_eq!(
            peer.state
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(name, _)| name == "sessions_list")
                .count(),
            native_calls
        );
        peer.finish().await;
    }
}

#[tokio::test]
async fn any_reads_provider_pages_without_inventing_native_model_or_path_metadata() {
    let peer = ReadPeer::start(ReplyScenario::ProviderRows).await;
    let snapshot = crate::test_support::complete_source_reader::read_complete_source(
        peer.loader(),
        peer.request(),
    )
    .await
    .into_snapshot()
    .unwrap();
    let methods = peer
        .state
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    assert!(methods.iter().any(|name| name == "provider_sessions_list"));
    assert_eq!(snapshot.records.len(), 1);
    assert!(
        matches!(&snapshot.records[0].identity,SessionPickerIdentity::HostedProvider(target) if String::from(target.endpoint.endpoint_id.clone())=="claude-local")
    );
    assert_eq!(snapshot.records[0].cwd.as_deref(), Some("/source/provider"));
    assert!(
        snapshot.records[0].model.is_none()
            && snapshot.records[0].reasoning_effort.is_none()
            && snapshot.records[0].normalized_cwd.is_none()
    );
    peer.finish().await;
}

#[tokio::test]
async fn canceling_the_actual_dispatch_read_prevents_a_late_reply_or_final_epoch_probe() {
    let peer = ReadPeer::start(ReplyScenario::HoldPage).await;
    let read = tokio::spawn(
        crate::test_support::complete_source_reader::read_complete_source(
            peer.loader(),
            peer.request(),
        ),
    );
    tokio::time::timeout(Duration::from_secs(2), peer.state.native_started.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    read.abort();
    assert!(read.await.unwrap_err().is_cancelled());
    assert_eq!(
        peer.state
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "endpoints_list")
            .count(),
        1
    );
    peer.finish().await;
}

#[tokio::test]
async fn complete_limits_and_sparse_opaque_pages_preserve_every_row_and_request_cursor() {
    for (scenario, row_count, page_count) in [
        (ReplyScenario::RowBudget(4096), 4096, 41),
        (ReplyScenario::PageBudget { complete: true }, 0, 65),
        (ReplyScenario::SparsePages, 1, 3),
    ] {
        let peer = ReadPeer::start(scenario).await;
        let snapshot = crate::test_support::complete_source_reader::read_complete_source(
            peer.loader(),
            peer.request(),
        )
        .await
        .into_snapshot()
        .unwrap();
        assert_eq!(snapshot.records.len(), row_count);
        {
            let calls = peer.state.calls.lock().unwrap();
            let pages = calls
                .iter()
                .filter(|(method, _)| method == "sessions_list")
                .collect::<Vec<_>>();
            assert_eq!(pages.len(), page_count);
            assert!(pages[0].1.get("cursor").is_none_or(Value::is_null));
            for (index, page) in pages.iter().enumerate().skip(1) {
                assert_eq!(page.1["cursor"], format!("opaque +/=?☀ {index}"));
                assert_eq!(page.1["endpoint"]["serviceId"], SERVICE);
                assert_eq!(page.1["view"], "stored");
            }
        }
        assert_eq!(peer.inventory_calls(), page_count + 1);
        peer.finish().await;
    }
}

#[path = "configured_inventory_continuation_tests.rs"]
mod continuation_tests;

#[path = "configured_inventory_native.rs"]
mod native_partition_tests;
