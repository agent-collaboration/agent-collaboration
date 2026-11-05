//! A genuine unrelated public write must never pass an exact source comparison.
use super::{
    cutover_host_fixture::CutoverHostFixture, offline_cli_fixture::TestResult,
    recovery_rows::require,
};
use board_staging_cutover::PreparedProjectBundle;
use collaboration_client::CollaborationClient;
use message_board::public_replies::{RequestDecision, RequestResult};
use message_board::{
    BoardOperation, Identity, ProjectRequest, ProtocolVersion, RequestId, TrailSubject,
    WorkTrailRowId,
};

pub(super) async fn append(
    host: &CutoverHostFixture,
    bundle: &PreparedProjectBundle,
) -> TestResult {
    let directory = codex_router_host::router_collaboration_directory(&host.root);
    let client =
        CollaborationClient::connect(&directory, "unexplained-write-regression", "1").await?;
    let response = client
        .work_trail_add(ProjectRequest {
            request_id: RequestId::generate(),
            project_id: bundle.manifest().source.selected.project.clone(),
            actor: Identity::Human {
                human_id: bundle.manifest().header.declared_owner.clone(),
            },
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: BoardOperation::WorkTrailAdd {
                row_id: WorkTrailRowId::generate(),
                subject: Some(TrailSubject::Project),
                text: "unexplained post-cutover write".to_owned().try_into()?,
                tags: vec![],
                links: vec![],
            },
        })
        .await?;
    require(
        matches!(
            response.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "the real unrelated application write was not accepted",
    )?;
    Ok(())
}

pub(super) async fn append_rejected_request(
    host: &CutoverHostFixture,
    bundle: &PreparedProjectBundle,
) -> TestResult {
    let directory = codex_router_host::router_collaboration_directory(&host.root);
    let client =
        CollaborationClient::connect(&directory, "unexplained-decision-regression", "1").await?;
    let response = client
        .task_update(ProjectRequest {
            request_id: RequestId::generate(),
            project_id: bundle.manifest().source.selected.project.clone(),
            actor: Identity::Human {
                human_id: bundle.manifest().header.declared_owner.clone(),
            },
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: BoardOperation::TaskUpdate {
                task_id: message_board::TaskId::generate(),
                title: "nonexistent task".to_owned().try_into()?,
                body: "this unrelated rejection must be accounted for".to_owned(),
                expected_revision: message_board::SubjectRevision::INITIAL,
            },
        })
        .await?;
    require(
        matches!(
            response.result,
            RequestResult::Decided(RequestDecision::Rejected { .. })
        ),
        "unrelated request did not produce a real rejected decision",
    )?;
    Ok(())
}
