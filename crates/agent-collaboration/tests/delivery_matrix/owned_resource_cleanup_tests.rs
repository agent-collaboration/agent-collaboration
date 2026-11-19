//! Pure failure-projection cases; no client, service, store or runtime is invoked.
use super::super::proof_context::ProofResult;
use super::CleanupOutcome;
use serde_json::json;

#[test]
fn board_failure_projection_keeps_typed_rejection_separate_from_unknown_write() -> ProofResult<()> {
    use super::{CleanupUncertainty, ReportedCleanupFailure};
    use collaboration_client::{
        BoardClientError,
        board::{
            BoardError, BoardFailureKind, BoardFailureStage, BoardNextAction, ResourceIdentity,
            ThreadId,
        },
    };
    let root_message_id = ThreadId::try_from("019f0000-0000-7000-8000-000000009014".to_owned())?;
    let resource = ResourceIdentity::Thread {
        thread_id: root_message_id,
    };
    let rejected = super::project_board_failure(BoardClientError::Rejected(Box::new(
        BoardError::resource_not_found(resource.clone()),
    )));
    if !matches!(
        rejected,
        CleanupOutcome::Rejected(ReportedCleanupFailure::Board {
            kind: BoardFailureKind::ResourceNotFound,
            stage: BoardFailureStage::Inspection,
        })
    ) {
        return Err("board rejection lost actual kind/stage or became unknown/success".into());
    }
    let unknown = super::project_board_failure(BoardClientError::OutcomeUnknown {
        resource,
        message: "UNRETAINED-SYNTHETIC-BOARD-TEXT",
        next_action: BoardNextAction::InspectResource,
    });
    if !matches!(
        unknown,
        CleanupOutcome::Unknown(CleanupUncertainty::BoardOutcomeUnknown)
    ) {
        return Err(
            "uncertain board write was laundered into rejected/returned/no-effect evidence".into(),
        );
    }
    Ok(())
}

#[test]
fn client_failure_projection_preserves_transport_kind_and_exact_rpc_code() -> ProofResult<()> {
    use super::{CleanupUncertainty, ClientFailureKind};
    use collaboration_client::ClientError;
    let transport = super::project_client_failure(ClientError::Transport(
        std::io::ErrorKind::ConnectionReset.into(),
    ));
    if !matches!(
        transport,
        CleanupOutcome::Unknown(CleanupUncertainty::Client(ClientFailureKind::Transport(
            std::io::ErrorKind::ConnectionReset
        ),))
    ) {
        return Err(
            "transport uncertainty lost its closed I/O category or became a rejection".into(),
        );
    }
    let rpc = super::project_client_failure(ClientError::Rejected {
        code: -32603,
        data: Some(json!({"privateText":"UNRETAINED-SYNTHETIC-RPC-TEXT"})),
    });
    if !matches!(
        &rpc,
        CleanupOutcome::Unknown(CleanupUncertainty::Client(ClientFailureKind::RpcRejected {
            code: -32603
        }))
    ) {
        return Err(
            "RPC cleanup uncertainty lost its exact signed code or became a returned outcome"
                .into(),
        );
    }
    if format!("{rpc:?}").contains("UNRETAINED") {
        return Err("client projection retained arbitrary RPC data".into());
    }
    Ok(())
}

#[path = "owned_resource_cleanup_runtime_tests.rs"]
mod runtime_tests;
