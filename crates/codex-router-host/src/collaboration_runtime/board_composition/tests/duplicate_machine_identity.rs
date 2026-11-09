//! The real central must explain replacement before closing the old physical socket.
use super::*;
use board_fleet_sync::{BoardProjectSchema, ReqwestSessionConnection, ReqwestSessionTransport};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_protocol::{ClientFrame, CloseReason, SchemaSupport, ServerFrame, SessionId};
use fleet_session::{SessionTransport, TransportConnection};

enum Replacement {
    SameSession,
    NewSession,
}

#[tokio::test]
async fn duplicate_connection_reports_connection_replaced_before_physical_close() -> TestResult {
    verify_replacement(Replacement::SameSession).await
}

#[tokio::test]
async fn duplicate_peer_with_new_session_reports_session_ended_before_physical_close() -> TestResult
{
    verify_replacement(Replacement::NewSession).await
}

async fn verify_replacement(replacement: Replacement) -> TestResult {
    let directory = tempfile::tempdir()?;
    let central = board_central_test_support::TestCentral::start(
        &directory.path().join("central"),
        &origin()?,
    )
    .await?;
    let observed: TestResult = async {
        let inputs = central.session_inputs()?;
        let transport = ReqwestSessionTransport::new(inputs.access, BOUND, BOUND)?;
        let mut original = transport
            .connect(&inputs.endpoint, &inputs.credential)
            .await?;
        let original_session = SessionId::new(uuid::Uuid::now_v7())?;
        hello(&mut original, original_session).await?;
        require_welcome(&mut original, original_session).await?;
        let (replacement_session, expected_reason) = match replacement {
            Replacement::SameSession => (original_session, CloseReason::ConnectionReplaced),
            Replacement::NewSession => (
                SessionId::new(uuid::Uuid::now_v7())?,
                CloseReason::SessionEnded,
            ),
        };
        let mut newer = transport
            .connect(&inputs.endpoint, &inputs.credential)
            .await?;
        hello(&mut newer, replacement_session).await?;
        require_welcome(&mut newer, replacement_session).await?;
        let bytes = timeout(BOUND, original.receive())
            .await?
            .ok_or("old socket closed without an explanatory frame")??;
        let closed = serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?;
        if !matches!(closed, ServerFrame::Closed { reason } if reason == expected_reason) {
            return Err("old socket did not retain the exact replacement reason".into());
        }
        if timeout(BOUND, original.receive()).await?.is_some() {
            return Err("old socket stayed open after its replacement frame".into());
        }
        newer.close(CloseReason::ClientClosed).await;
        Ok(())
    }
    .await;
    let stopped = central.shutdown().await;
    observed?;
    stopped
}

async fn hello(connection: &mut ReqwestSessionConnection, session_id: SessionId) -> TestResult {
    let max_version = BoardProjectSchema::new()?
        .supported_schemas()
        .iter()
        .map(|schema| schema.version)
        .max()
        .ok_or("empty board schema")?;
    connection
        .send(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id,
                schema_support: SchemaSupport { max_version },
            })?
            .into(),
        )
        .await?;
    Ok(())
}

async fn require_welcome(
    connection: &mut ReqwestSessionConnection,
    expected: SessionId,
) -> TestResult {
    let bytes = timeout(BOUND, connection.receive())
        .await?
        .ok_or("Welcome missing")??;
    let frame = serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?;
    if !matches!(frame, ServerFrame::Welcome { session_id, .. } if session_id == expected) {
        return Err("central did not welcome the requested session".into());
    }
    Ok(())
}
