//! Reuse the G1 native publication fixture to test the Host's local restart repair.
use super::{TestResult, identity};
use crate::collaboration_runtime::board_composition::open_board_admission;
use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use fleet_sync_protocol::ReceiverId;
use fleet_sync_session::{Endpoint, PeerCredential};
use message_board::*;
use std::{io, sync::Arc};

// The shared harness also contains unrelated journey helpers.

#[path = "../../../../../collaboration-service/tests/support/accepted_snapshot.rs"]
mod accepted_snapshot;

#[path = "../../../../../collaboration-service/tests/support/board_fixture.rs"]
pub(super) mod board_fixture;

#[tokio::test]
async fn snapshot_baseline_is_repaired_before_offline_admission_without_waiting_for_welcome()
-> TestResult {
    let source = board_fixture::BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let request: ProjectRequest = serde_json::from_value(source.request(
        &project,
        serde_json::json!({
            "kind":"workstreamCreate", "workstreamId":WorkstreamId::generate(),
            "name":"baseline restart", "goal":"reuse the original admitted request",
        }),
    ))?;
    let original = source._admission.submit(&request).await?;
    assert!(matches!(
        original.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let target = board_fixture::BoardFixture::start().await?;
    accepted_snapshot::bootstrap_unprepared_project(&source, &target, &project).await?;
    let root = target.stop_and_keep_directory().await?;
    let board = open_board_admission(
        root.path(),
        identity()?,
        Some(BoardSessionInputs {
            endpoint: Endpoint::new("ws://127.0.0.1:1/session")?,
            access: AccessServiceToken::new("fixture-access", "fixture-secret")?,
            credential: PeerCredential::new("Bearer offline-fixture")?,
            peer: board_fixture::fixture_peer()?,
            relay: "0192a1b2-c3d4-7000-8000-000000000003".parse()?,
            receiver: ReceiverId::new(durable_steps::Uuid::now_v7())?,
        }),
    )
    .await?;
    // Do not start the pump or call ensure from this test: either would hide a
    // constructor that left the baseline repair behind the Welcome boundary.
    assert_eq!(
        board.admission.submit(&request).await?.result,
        original.result
    );
    let admission = board.admission.clone();
    // The observation above precedes pump activation. Use the normal owner
    // shutdown path to release prepared per-project handles before closing admission.
    board
        .start()
        .ok_or("prepared session missing")?
        .shutdown()
        .await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("snapshot admission still owned"))?
        .close()
        .await?;
    source.stop().await?;
    Ok(())
}
