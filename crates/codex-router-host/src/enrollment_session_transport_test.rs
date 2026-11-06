use super::*;
use board_central_test_support::TestCentral;
use fleet_sync_protocol::CloseReason;
use fleet_sync_session::TransportConnection;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
async fn pending_reenrollment_blocks_the_next_real_socket_until_restart_finishes_reset()
-> TestResult {
    // Arrange the actual Access-verified WebSocket, with no native/model input.
    let root = tempfile::tempdir()?;
    let origin = message_board::ActorOrigin {
        installation: message_board::ServiceId::try_from(uuid::Uuid::now_v7().to_string())?,
        machine: message_board::MachineLabel::try_from("enrollment guard fixture".to_owned())?,
    };
    let central = TestCentral::start(&root.path().join("central"), &origin).await?;
    let journal = crate::router_collaboration_directory(root.path());
    let storage = root.path().join("custom-storage");
    let inputs = central.session_inputs()?;
    let transport = EnrollmentSessionTransport::new(
        ReqwestSessionTransport::new(
            inputs.access,
            Duration::from_secs(10),
            Duration::from_secs(10),
        )?,
        &journal,
    );
    let first = transport
        .connect(&inputs.endpoint, &inputs.credential)
        .await?;
    first.close(CloseReason::ClientClosed).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while central.session_connections() < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(central.session_connections(), 1);

    // Act: the CLI's pending marker appears while this transport owner is still live.
    drop(crate::enrollment_reset::record_pending_enrollment_reset(
        &journal,
    )?);

    // Assert: neither pending nor malformed reset state opens another socket.
    assert!(matches!(
        transport
            .connect(&inputs.endpoint, &inputs.credential)
            .await,
        Err(ConnectFailure::Unavailable)
    ));
    assert_eq!(central.session_connections(), 1);
    assert!(crate::fleet_enrollment_restart_required(root.path())?);
    let pending = tokio::fs::read(journal.join("enrollment-reset.json")).await?;
    tokio::fs::write(journal.join("enrollment-reset.json"), b"invalid journal").await?;
    assert!(matches!(
        transport
            .connect(&inputs.endpoint, &inputs.credential)
            .await,
        Err(ConnectFailure::Unavailable)
    ));
    assert_eq!(central.session_connections(), 1);
    tokio::fs::write(journal.join("enrollment-reset.json"), pending).await?;
    crate::enrollment_reset::resume_enrollment_reset(&journal, &storage).await?;
    assert!(!crate::fleet_enrollment_restart_required(root.path())?);
    let restarted = transport
        .connect(&inputs.endpoint, &inputs.credential)
        .await?;
    restarted.close(CloseReason::ClientClosed).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while central.session_connections() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(central.session_connections(), 2);
    central.shutdown().await?;
    Ok(())
}
