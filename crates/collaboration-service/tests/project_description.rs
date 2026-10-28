//! Exact descriptions use project admission, current/feed decoding and public reads.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
#[path = "support/project_description.rs"]
mod project_description;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::public_replies::{RequestDecision, RequestResult};
use message_board::*;
use message_board_storage::ProjectStorageError;
use project_description::{create_project, description_with_control_characters, project_state};
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

fn local_state(shown: ProjectShowResult) -> Result<ProjectState, TestError> {
    match shown.state {
        ProjectStateView::Local(state) => Ok(*state),
        ProjectStateView::NotLocal => Err("local state reported notLocal".into()),
    }
}

fn editor() -> Result<Identity, TestError> {
    Ok(serde_json::from_value(json!({"kind":"session","session":{
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"fixture-description-editor-without-a-role"
    }}))?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn descriptions_round_trip_through_create_describe_show_title_update_and_reopen()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let original = description_with_control_characters(0);
    assert_eq!(original.len(), 16 * 1024);
    let (project, _) = create_project(&fixture, Some(&original)).await?;
    let client = fixture.api.client("description round trip").await?;
    let shown = client
        .project_show(ProjectShowRequest {
            project_id: project.clone(),
        })
        .await?;
    assert_eq!(shown.project.title.as_str(), "described project");
    assert!(
        serde_json::to_value(&shown.project)?
            .get("description")
            .is_none(),
        "description must not enter the directory contract"
    );
    let initial = local_state(shown)?;
    assert_eq!(initial.description.as_str(), original);
    assert_eq!(initial, project_state(&fixture, &project).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert!(read.records(RecordKind::RoleAssignment).await?.is_empty());
    read.finish().await?;

    // This declared session has no project role: descriptions carry no authority rule.
    let updated = " \nupdated\0\t description  ";
    let request = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: project.clone(),
        actor: editor()?,
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::ProjectDescribe {
            description: Description::try_from(updated.to_owned())?,
            expected_revision: initial.revision,
        },
    };
    let reply = client.project_describe(request.clone()).await?;
    assert!(matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let changed = local_state(
        client
            .project_show(ProjectShowRequest {
                project_id: project.clone(),
            })
            .await?,
    )?;
    assert_eq!(changed.description.as_str(), updated);
    assert_eq!(changed.revision.get(), initial.revision.get() + 1);
    assert_eq!(
        changed.envelope.actor.declared_identity(),
        Some(request.actor.clone())
    );

    let mut stale = request.clone();
    stale.request_id = RequestId::generate();
    let refused = client.project_describe(stale).await?;
    assert!(
        matches!(refused.result, RequestResult::Decided(RequestDecision::Rejected { reasons })
        if reasons == vec![Refusal::StaleRevision { current: changed.revision }])
    );
    assert_eq!(project_state(&fixture, &project).await?, changed);
    let read = store.snapshot().await?;
    let before_replay = read.high_water();
    read.finish().await?;
    assert_eq!(client.project_describe(request).await?.result, reply.result);
    let read = store.snapshot().await?;
    assert_eq!(
        read.high_water(),
        before_replay,
        "keyed retry writes no new version"
    );
    read.finish().await?;

    // Title changes stay fleet-owned and do not write a project-state version.
    client
        .project_update(ProjectUpdateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: project.clone(),
            title: ResourceName::try_from("renamed project".to_owned())?,
            actor: serde_json::from_value(fixture.actor.clone())?,
            acting_for: None,
        })
        .await?;
    let shown = client
        .project_show(ProjectShowRequest {
            project_id: project.clone(),
        })
        .await?;
    assert_eq!(shown.project.title.as_str(), "renamed project");
    assert_eq!(local_state(shown)?, changed);
    drop(store);
    drop(client);
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("reopened description").await?;
    assert_eq!(
        local_state(
            client
                .project_show(ProjectShowRequest {
                    project_id: project
                })
                .await?
        )?,
        changed
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn omitted_and_explicit_empty_descriptions_share_the_creation_retry_hash()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, mut request) = create_project(&fixture, None).await?;
    let first = fixture.api.call("project_create", request.clone()).await?;
    accepted(&first);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    request
        .as_object_mut()
        .ok_or("creation request")?
        .insert("description".to_owned(), json!(""));
    let repeated = fixture.api.call("project_create", request.clone()).await?;
    assert_eq!(repeated.get("result"), first.get("result"));
    assert_eq!(
        project_state(&fixture, &project)
            .await?
            .description
            .as_str(),
        ""
    );
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), before);
    read.finish().await?;
    request
        .as_object_mut()
        .ok_or("creation request")?
        .insert("description".to_owned(), json!("different intent"));
    let conflict = fixture.api.call("project_create", request).await?;
    assert_eq!(
        conflict.pointer("/result/result/kind"),
        Some(&json!("conflictingRequest"))
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_directory_only_project_is_not_local_and_show_creates_no_file() -> Result<(), TestError> {
    use fleet_admission::{FleetOperation, FleetRequest};
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let directory = fixture
        ._directory
        .path()
        .join("projects")
        .join(project.as_str());
    let reply = fixture
        ._fleet
        .submit(
            &FleetRequest {
                request_id: RequestId::generate(),
                workspace: fixture.workspace.clone(),
                actor: RecordedActor::declared(serde_json::from_value(fixture.actor.clone())?),
                acting_for: None,
                protocol_version: ProtocolVersion::CURRENT,
                operation: FleetOperation::ProjectDirectoryCreate {
                    project: project.clone(),
                    board: fixture.board.clone(),
                    title: ResourceName::try_from("directory only".to_owned())?,
                },
            },
            &fixture.record_origin()?,
        )
        .await?;
    assert!(matches!(
        reply.result,
        message_board::RequestResult::Decided(message_board::RequestDecision::Accepted { .. })
    ));
    assert!(!directory.exists());
    let client = fixture.api.client("directory-only state").await?;
    let shown = client
        .project_show(ProjectShowRequest {
            project_id: project.clone(),
        })
        .await?;
    assert_eq!(shown.project.id, project);
    assert_eq!(shown.state, ProjectStateView::NotLocal);
    assert!(
        !directory.exists(),
        "read must not prepare a project writer"
    );
    fixture.stop().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn assert_invalid<T>(
    result: Result<T, ProjectStorageError>,
    project: &ProjectId,
    expected: &str,
) -> Result<(), TestError> {
    match result {
        Err(ProjectStorageError::InvalidRecord { record, field }) => {
            assert_eq!(record, project.clone().into());
            assert_eq!(field, expected);
            Ok(())
        }
        Err(error) => {
            Err(format!("description lost its typed stored-value refusal: {error}").into())
        }
        Ok(_) => Err("oversize description was accepted".into()),
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn current_and_feed_descriptions_are_strict_and_never_silently_truncated()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, creation) = create_project(&fixture, None).await?;
    for text in ["x".repeat(16 * 1024 + 1), "é".repeat(8 * 1024 + 1)] {
        let mut invalid_request = creation.clone();
        invalid_request
            .as_object_mut()
            .ok_or("request")?
            .insert("description".to_owned(), json!(text));
        assert!(serde_json::from_value::<ProjectCreateRequest>(invalid_request).is_err());
    }
    let state = project_state(&fixture, &project).await?;
    let mut missing = serde_json::to_value(state)?;
    assert!(
        missing
            .as_object_mut()
            .ok_or("state")?
            .remove("description")
            .is_some()
    );
    assert!(
        serde_json::from_value::<ProjectState>(missing).is_err(),
        "stored state has no compatibility default"
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let oversized = "x".repeat(16 * 1024 + 1);
    assert_eq!(
        sqlx::query("UPDATE project_state SET description=? WHERE project_id=?")
            .bind(&oversized)
            .bind(project.as_str())
            .execute(&mut observer)
            .await?
            .rows_affected(),
        1
    );
    let mut read = store.snapshot().await?;
    assert_invalid(
        read.record(&project.clone().into()).await,
        &project,
        "description",
    )?;
    assert_invalid(
        read.records(RecordKind::Project).await,
        &project,
        "description",
    )?;
    read.finish().await?;
    assert_eq!(
        sqlx::query("UPDATE project_state SET description='' WHERE project_id=?")
            .bind(project.as_str())
            .execute(&mut observer)
            .await?
            .rows_affected(),
        1
    );
    let mut read = store.snapshot().await?;
    let zero = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::ZERO,
    };
    let FeedRead::Entries { entries, .. } = read.feed_after(zero, u64::MAX, true).await? else {
        return Err("feed unavailable".into());
    };
    let entry = entries
        .into_iter()
        .find(|entry| entry.record == project.clone().into())
        .ok_or("project feed version")?;
    read.finish().await?;
    let original = serde_json::to_value(entry.content)?;
    let mut corrupt = original.clone();
    let description = corrupt
        .pointer_mut("/record/record/description")
        .ok_or("stored description")?;
    assert_eq!(description, &json!(""));
    *description = json!(oversized);
    assert_eq!(
        sqlx::query(
            "UPDATE project_feed SET content=? WHERE record_kind='project' AND record_id=?"
        )
        .bind(corrupt.to_string())
        .bind(project.as_str())
        .execute(&mut observer)
        .await?
        .rows_affected(),
        1
    );
    let mut read = store.snapshot().await?;
    assert_invalid(
        read.feed_after(zero, u64::MAX, true).await,
        &project,
        "content",
    )?;
    read.finish().await?;
    assert_eq!(
        sqlx::query(
            "UPDATE project_feed SET content=? WHERE record_kind='project' AND record_id=?"
        )
        .bind(original.to_string())
        .bind(project.as_str())
        .execute(&mut observer)
        .await?
        .rows_affected(),
        1
    );
    observer.close().await?;
    assert_eq!(
        project_state(&fixture, &project)
            .await?
            .description
            .as_str(),
        ""
    );
    fixture.stop().await
}
