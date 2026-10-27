#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Central-first creation gates and initialization after an explicit registered-state input.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};
use message_board::*;

fn request(fixture: &BoardFixture) -> Result<ProjectCreateRequest, TestError> {
    Ok(ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: ProjectId::generate(),
        board_id: fixture.board.clone(),
        title: ResourceName::try_from("creation gate".to_owned())?,
        description: Some(Description::try_from(
            " exact initial description\nwith a second line ".to_owned(),
        )?),
        actor: serde_json::from_value(fixture.actor.clone())?,
        acting_for: None,
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_directory_request_without_central_acceptance_creates_no_native_file()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_offline().await?;
    let request = request(&fixture)?;
    let result = fixture._admission.create_project(&request).await;
    let project_directory = fixture
        ._directory
        .path()
        .join("projects")
        .join(request.project_id.as_str());
    assert!(
        !project_directory.exists(),
        "creation opened a native file before central accepted: {result:?}"
    );
    assert!(
        fixture
            ._fleet
            .pending_request(&request.request_id)
            .await?
            .is_some(),
        "creating request was not durably queued"
    );
    assert_eq!(result?.result, RequestResult::PendingUnreachable);
    assert_eq!(
        fixture._fleet.creation_input(&request.request_id).await?,
        Some(request.clone())
    );
    let mut conflict = request.clone();
    conflict.description = Some(Description::try_from("changed while pending".to_owned())?);
    assert_eq!(
        fixture._admission.create_project(&conflict).await?.result,
        RequestResult::ConflictingRequest
    );
    assert_eq!(
        fixture._fleet.creation_input(&request.request_id).await?,
        Some(request.clone())
    );
    let fixture = fixture.restart_offline_read_fixture().await?;
    assert_eq!(
        fixture._fleet.creation_input(&request.request_id).await?,
        Some(request.clone())
    );
    fixture._admission.recover_creating_projects().await?;
    assert!(!project_directory.exists());
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn accepted_directory_survives_reopen_and_pending_registration_records_nothing()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_offline().await?;
    let request = request(&fixture)?;
    assert_eq!(
        fixture._admission.create_project(&request).await?.result,
        RequestResult::PendingUnreachable
    );
    // Reconnect the real owning central to supply the directory decision.
    let fixture = fixture.restart_read_fixture().await?;
    board_fixture::settle_accepted_creation(&fixture._fleet, &request, &fixture.record_origin()?)
        .await?;
    // Crash after the directory decision, before a Host has bootstrapped the native file.
    let fixture = fixture.restart_read_fixture().await?;
    assert_eq!(
        fixture._fleet.accepted_creation_page("", 100).await?,
        vec![request.clone()]
    );
    fixture._admission.recover_creating_projects().await?;
    let path = fixture
        ._directory
        .path()
        .join("projects")
        .join(request.project_id.as_str());
    assert!(!path.exists());
    let store = fixture._registry.open_project(&request.project_id).await?;
    assert_eq!(
        fixture._admission.create_project(&request).await?.result,
        RequestResult::ProjectCreationPending {
            project: request.project_id.clone()
        },
        "durable accepted creation must not report an error while native registration is pending"
    );
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water().position, FeedPosition::ZERO);
    assert!(
        read.record(&request.project_id.clone().into())
            .await?
            .is_none()
    );
    assert!(
        read.record(&request.request_id.clone().into())
            .await?
            .is_none()
    );
    read.finish().await?;
    drop(store);
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn directory_refusal_keeps_its_structured_reason_without_creating_a_file()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_offline().await?;
    let request = request(&fixture)?;
    fixture._admission.create_project(&request).await?;
    let refusal = Refusal::NameConflict {
        name: request.title.clone(),
        existing: ProjectId::generate().into(),
    };
    fixture
        ._fleet
        .settle_central_reply(
            &request.request_id,
            &RequestDecision::Rejected {
                reasons: vec![refusal.clone()],
            },
        )
        .await?;
    let expected = RequestResult::ScopeUnavailable(ScopeRefusal::ProjectRejected {
        project: request.project_id.clone(),
        reason: ProjectRejection::Directory { refusal },
    });
    assert_eq!(
        fixture._admission.create_project(&request).await?.result,
        expected
    );
    assert!(
        !fixture
            ._directory
            .path()
            .join("projects")
            .join(request.project_id.as_str())
            .exists()
    );
    let fixture = fixture.restart_offline_read_fixture().await?;
    assert_eq!(
        fixture._admission.create_project(&request).await?.result,
        expected
    );
    assert!(
        fixture
            ._fleet
            .accepted_creation_page("", 100)
            .await?
            .is_empty()
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn ready_registration_recovery_uses_complete_input_and_the_creating_ledger_once()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_offline().await?;
    let request = request(&fixture)?;
    fixture._admission.create_project(&request).await?;
    let fixture = fixture.restart_read_fixture().await?;
    fixture.prepare_registered_creation(&request).await?;
    // Crash after real native Ready, before the first application transaction.
    let fixture = fixture.restart_read_fixture().await?;
    fixture._admission.recover_creating_projects().await?;
    let store = fixture
        ._registry
        .existing_project(&request.project_id)
        .await?
        .ok_or("recovered store")?;
    let mut read = store.snapshot().await?;
    let Some(Record::Project(state)) = read.record(&request.project_id.clone().into()).await?
    else {
        return Err("initial state missing".into());
    };
    assert_eq!(Some(&state.description), request.description.as_ref());
    assert_eq!(
        state
            .envelope
            .position
            .project_cursor()
            .ok_or("placement")?
            .position
            .get(),
        1
    );
    assert_eq!(read.high_water().position.get(), 3);
    assert_eq!(read.records(RecordKind::Fact).await?.len(), 1);
    assert_eq!(read.records(RecordKind::Request).await?.len(), 1);
    let head = read.high_water();
    read.finish().await?;
    let first = fixture._admission.create_project(&request).await?;
    drop(store);
    // Crash after the creating ledger, before any outbox cleanup; input remains safe to replay.
    let fixture = fixture.restart_read_fixture().await?;
    let (first_recovery, second_recovery) = tokio::join!(
        fixture._admission.recover_creating_projects(),
        fixture
            ._admission
            .resume_creating_project(&request.project_id),
    );
    first_recovery?;
    second_recovery?;
    assert_eq!(
        fixture._admission.create_project(&request).await?.result,
        first.result
    );
    let store = fixture
        ._registry
        .existing_project(&request.project_id)
        .await?
        .ok_or("replayed store")?;
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), head);
    read.finish().await?;
    drop(store);
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn registered_creation_preserves_initial_state_and_replays_after_reopen_without_effects()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let request = request(&fixture)?;
    // Directory acceptance and the typed native grant are controlled component
    // inputs. The full pending/restart creation journey is a separate obligation.
    fixture.prepare_registered_creation(&request).await?;
    let first = fixture._admission.create_project(&request).await?;
    assert!(matches!(
        &first.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let store = fixture
        ._registry
        .existing_project(&request.project_id)
        .await?
        .ok_or("created store missing")?;
    let mut read = store.snapshot().await?;
    let Some(Record::Project(state)) = read.record(&request.project_id.clone().into()).await?
    else {
        return Err("initial state missing".into());
    };
    assert_eq!(Some(&state.description), request.description.as_ref());
    assert_eq!(
        state
            .envelope
            .position
            .project_cursor()
            .ok_or("state placement")?
            .position
            .get(),
        1
    );
    assert!(read.records(RecordKind::Workstream).await?.is_empty());
    assert!(read.records(RecordKind::RoleAssignment).await?.is_empty());
    let head = read.high_water();
    read.finish().await?;
    assert_eq!(
        fixture._admission.create_project(&request).await?.result,
        first.result
    );
    let mut changed = request.clone();
    changed.description = Some(Description::try_from(
        "different creation input".to_owned(),
    )?);
    assert_eq!(
        fixture._admission.create_project(&changed).await?.result,
        RequestResult::ConflictingRequest
    );
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), head);
    read.finish().await?;
    drop(store);
    let reopened = fixture.restart_read_fixture().await?;
    assert_eq!(
        reopened._admission.create_project(&request).await?.result,
        first.result
    );
    let store = reopened
        ._registry
        .existing_project(&request.project_id)
        .await?
        .ok_or("reopened store missing")?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), head);
    assert!(
        matches!(read.record(&request.project_id.clone().into()).await?, Some(Record::Project(state)) if Some(&state.description) == request.description.as_ref())
    );
    read.finish().await?;
    drop(store);
    reopened.stop().await
}
