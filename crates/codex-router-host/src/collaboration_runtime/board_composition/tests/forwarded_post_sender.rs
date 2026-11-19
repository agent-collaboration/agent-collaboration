//! A sender's board entry uses the real machine session, central and holder ledger.
use super::*;
use collaboration_service::ApplicationFixture;

#[cfg(feature = "runtime-test-support")]
#[path = "non_holder_task_writes.rs"]
mod non_holder_task_writes;
#[cfg(feature = "runtime-test-support")]
#[path = "forwarded_post_notifications.rs"]
mod notifications;
#[path = "forwarded_post_unknown.rs"]
mod unknown_answer;

fn post_request(
    project: &ProjectId,
) -> Result<ProjectRequest, Box<dyn std::error::Error + Send + Sync>> {
    // A declared actor is independent of the machine's verified provenance.
    Ok(serde_json::from_value(serde_json::json!({
        "requestId": RequestId::generate(), "projectId": project, "protocolVersion": 1,
        "actor": {"kind":"session","session":{"endpoint":{"serviceId":"550e8400-e29b-41d4-a716-446655440999","endpointId":"codex-local"},"sessionId":"forwarding-declared-session"}}, "actingFor": null,
        "operation": {"kind":"threadCreate","postId": PostId::generate(),
            "about":{"kind":"project","subject":{"line":"forwarding fixture"}},
            "text":"synthetic forwarding fixture", "references":[], "participation":"participant",
            "watch":false,"addressees":[]}
    }))?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn non_holder_sender_without_a_machine_session_refuses_without_queuing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let board = open_board_admission(directory.path(), identity()?, None).await?;
    let admission = board.admission.clone();
    assert!(board.start().session.is_none());
    let application = (ApplicationFixture::new(
        origin()?.installation.as_str(),
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone()))
    .build();
    let proof = async {
        let reply = application
            .board()
            .project_write(post_request(&ProjectId::generate())?)
            .await?;
        assert!(matches!(
            reply.result,
            message_board::public_replies::RequestResult::PendingUnreachable
        ));
        assert!(reply.reader.is_none());
        #[cfg(feature = "runtime-test-support")]
        assert!(
            admission
                .runtime_test_context()
                .fleet
                .pending_requests()
                .await?
                .is_empty()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(application);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("unattached admission still owned"))?
        .close()
        .await;
    proof?;
    closed?;
    Ok(())
}
