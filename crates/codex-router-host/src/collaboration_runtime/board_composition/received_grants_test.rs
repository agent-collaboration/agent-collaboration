//! An initial Registration and its own grant notice keep one created writer.
use crate::collaboration_runtime::board_composition::{
    open_board_admission,
    project_writers::NoticeHandling,
    tests::{identity, origin, wait_welcome},
};
use board_central_test_support::TestCentral;
use fleet_sync_protocol::{DatabaseId, Epoch};
use fleet_sync_writer::WriterRegistrationOutcome;
use message_board::{
    BoardId, HumanId, Identity, ProjectCreateRequest, ProjectId, ProtocolVersion, Record,
    RequestDecision, RequestId, RequestResult, ResourceName,
};
use std::{io, sync::Arc, time::Duration};
use tokio::time::timeout;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(30);

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn initial_create_grant_notice_preserves_pending_registration_and_first_record() -> TestResult
{
    // Arrange: both messages originate in the real Fleet registration transaction.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let central = TestCentral::start(&directory.path().join("central"), &origin()?).await?;
    let project = ProjectId::generate();
    let mut registration =
        central.hold_registration_answer(DatabaseId::try_from(project.as_str().to_owned())?)?;
    let mut notice = central.hold_granted_notice(project.clone(), Epoch::new(1)?)?;
    let board = open_board_admission(&root, identity()?, Some(central.session_inputs()?)).await?;
    let projects = board
        .session
        .as_ref()
        .ok_or("machine session")?
        .projects
        .clone();
    let mut completions = projects.notice_completions.subscribe();
    let registry = projects.registry()?;
    let fleet = board.fleet.upgrade().ok_or("Fleet client")?;
    let admission = board.admission.clone();
    let running = board.start().ok_or("machine runtime")?;
    wait_welcome(projects.welcomed.clone()).await?;
    fleet.synchronize().await?;
    let application = (collaboration_service::ApplicationFixture::new(
        "550e8400-e29b-41d4-a716-446655440000",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone()))
    .build();
    let workspace = message_board::WorkspaceId::generate();
    let board_id = BoardId::generate();
    let actor = Identity::Human {
        human_id: HumanId::try_from("create-overlap-owner".to_owned())?,
    };
    let workspace_reply = application
        .board()
        .workspace_create(serde_json::from_value(serde_json::json!({
            "requestId": RequestId::generate(), "protocolVersion": 1,
            "workspaceId": workspace, "name": "create overlap workspace", "description": "",
            "actor": actor, "actingFor": null,
        }))?)
        .await?;
    assert!(
        matches!(
            workspace_reply.result,
            message_board::public_replies::RequestResult::Decided(
                message_board::public_replies::RequestDecision::Accepted { .. }
            )
        ),
        "workspace fixture failed: {workspace_reply:?}"
    );
    let board_reply = application.board().board_create(serde_json::from_value(serde_json::json!({
        "requestId": RequestId::generate(), "protocolVersion": 1,
        "workspaceId": workspace, "boardId": board_id, "name": "create overlap board", "description": "",
        "kind": {"kind": "area"}, "actor": actor, "actingFor": null,
    }))?).await?;
    assert!(
        matches!(
            board_reply.result,
            message_board::public_replies::RequestResult::Decided(
                message_board::public_replies::RequestDecision::Accepted { .. }
            )
        ),
        "board fixture failed: {board_reply:?}"
    );
    drop(application);
    let request = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
        board_id,
        title: ResourceName::try_from("initial create overlapping grant".to_owned())?,
        description: None,
        actor,
        acting_for: None,
    };
    let creating = admission.clone();
    let input = request.clone();
    let created = tokio::spawn(async move { creating.create_project(&input).await });
    let proof = async {
        timeout(BOUND, registration.wait_held()).await??;
        let position = timeout(BOUND, notice.wait_held()).await??;
        let original = projects.ensure(&project).await?;
        assert_eq!(
            original.store.registration_outcome().await?,
            WriterRegistrationOutcome::Pending
        );
        let database = registry.database(&project).await?;
        assert!(database.installed_snapshot().await?.is_none());

        // Act: force the notice ahead of the original Registration answer.
        notice.release()?;
        let handled = timeout(BOUND, async {
            loop {
                let (actual, outcome) = completions.recv().await?;
                if actual == position {
                    return Ok::<_, tokio::sync::broadcast::error::RecvError>(outcome);
                }
            }
        })
        .await??;

        // Assert: the hint acknowledges without replacing or stopping its writer.
        assert_eq!(
            handled,
            NoticeHandling::Handled,
            "own grant notice stopped a pending created writer"
        );
        assert!(Arc::ptr_eq(&original, &projects.ensure(&project).await?));
        assert!(database.installed_snapshot().await?.is_none());
        assert!(
            crate::collaboration_runtime::board_composition::tests::attempt_record(
                &crate::collaboration_runtime::board_composition::tests::attempt_key(
                    &root,
                    &project,
                    Epoch::new(1)?
                )
            )
            .await?
            .is_none()
        );
        registration.release()?;
        assert!(matches!(
            timeout(BOUND, original.runtime.wait_registration()).await??,
            WriterRegistrationOutcome::Ready { .. }
        ));
        timeout(BOUND, async {
            loop {
                let mut snapshot = original.store.snapshot().await?;
                let receipt = snapshot.record(&request.request_id.clone().into()).await?;
                if let Some(Record::Request(receipt)) = receipt {
                    assert!(matches!(
                        snapshot.request_decision(&receipt).await?,
                        RequestDecision::Accepted { .. }
                    ));
                    assert!(matches!(
                        snapshot.record(&project.clone().into()).await?,
                        Some(Record::Project(_))
                    ));
                    snapshot.finish().await?;
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                snapshot.finish().await?;
                tokio::task::yield_now().await;
            }
        })
        .await??;
        let replay = admission.create_project(&request).await?;
        assert!(matches!(
            replay.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
        assert!(Arc::ptr_eq(&original, &projects.ensure(&project).await?));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    registration.release()?;
    drop(notice);
    if !created.is_finished() {
        created.abort();
    }
    let _ = created.await;
    let stopped = running.shutdown().await;
    drop(fleet);
    drop(projects);
    drop(registry);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await;
    let central_closed = central.shutdown().await;
    proof?;
    stopped?;
    closed?;
    central_closed?;
    Ok(())
}
