//! Counterexample: legal project positions can have wall-clock times in reverse order.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use collaboration_contracts::MachineIdentity;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::CollaborationApplication;
use collaboration_service::ProjectAdmission;
use collaboration_service::ProjectAdmissionProps;
use collaboration_service::ServiceIdentity;
use message_board::*;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn aggregate_continuation_keeps_every_post_when_router_wall_clock_moves_backwards()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let installation = ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?;
    let origin = ActorOrigin {
        installation: installation.clone(),
        machine: MachineLabel::try_from("aggregate proof".to_owned())?,
    };
    let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
        directory.path(),
        Arc::new(board_fixture::PendingRegistration),
        &origin,
    )
    .await?;
    let fleet = Arc::new(fleet);
    let registry = board_fixture::new_registry(directory.path())?;
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: fleet.clone(),
        identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(installation.as_str().to_owned())?,
            Some("aggregate proof"),
        )?,
    })?);
    let api = ServedCollaborationApi::start(
        directory.path(),
        CollaborationApplication::new(
            ServiceIdentity::new(
                installation.as_str(),
                "00000000-0000-4000-8000-000000000002",
            )?
            .with_project_admission(admission),
        ),
    )
    .await?;
    let actor = Identity::Human {
        human_id: HumanId::try_from("reader".to_owned())?,
    };
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let other_project = ProjectId::generate();
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,"actor":actor});
    let mut request = header();
    request["workspaceId"] = json!(workspace);
    request["name"] = json!("workspace");
    request["description"] = json!("");
    assert_accepted(&api.call("workspace_create", request).await?);
    let mut request = header();
    request["workspaceId"] = json!(workspace);
    request["boardId"] = json!(board);
    request["name"] = json!("board");
    request["description"] = json!("");
    request["kind"] = json!({"kind":"area"});
    assert_accepted(&api.call("board_create", request).await?);
    for (project, title) in [
        (&project, "clock regression"),
        (&other_project, "second project"),
    ] {
        let mut request = header();
        request["projectId"] = json!(project);
        request["boardId"] = json!(board);
        request["title"] = json!(title);
        board_fixture::prepare_registered_creation(
            &fleet,
            &registry,
            &serde_json::from_value(request.clone())?,
            &origin,
            &runtime_primitives::SystemClock,
        )
        .await?;
        assert_accepted(&api.call("project_create", request).await?);
    }
    let mut ids = Vec::new();
    let mut positions = Vec::new();
    for (project, time) in [
        (&project, "2026-10-12T00:02:00Z"),
        (&project, "2026-10-12T00:01:00Z"),
        (&other_project, "2026-10-12T00:01:30Z"),
        (&other_project, "2026-10-12T00:03:00Z"),
    ] {
        let store = registry
            .existing_project(project)
            .await?
            .ok_or("project missing")?;
        let post_id = PostId::generate();
        let thread_id = ThreadId::from(post_id.clone());
        let envelope = |id| -> Result<RecordEnvelope, Box<dyn std::error::Error + Send + Sync>> {
            Ok(RecordEnvelope {
                id,
                scope: RecordScope {
                    workspace: workspace.clone(),
                    board: Some(board.clone()),
                    project: Some(project.clone()),
                },
                actor: RecordedActor::declared(actor.clone()),
                request: None,
                position: RecordPlacement::Project(ProjectFeedEntry {
                    cursor: FeedCursor {
                        epoch: EpochNumber::try_from(1)?,
                        position: FeedPosition::ZERO,
                    },
                    origin: origin.clone(),
                }),
                time: RecordTime::Router(time.parse()?),
            })
        };
        let mut write = store.begin_admission(origin.clone()).await?;
        write
            .append_record(Record::Thread(Thread {
                envelope: envelope(thread_id.clone().into())?,
                thread_id: thread_id.clone(),
                about: ThreadSubject::Project {
                    line: "Fixture discussion".to_owned().try_into()?,
                },
                imported: false,
                state: ThreadState::Open,
                orchestrator: None,
                implementer: None,
            }))
            .await?;
        let post = Post {
            envelope: envelope(post_id.clone().into())?,
            post_id: post_id.clone(),
            board_id: board.clone(),
            project_id: project.clone(),
            placement: Placement::Project {
                project_id: project.clone(),
            },
            actor: actor.clone(),
            acting_for: None,
            text: PostText::try_from(time.to_owned())?,
            references: PostReferences::try_from(Vec::new())?,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: FeedCursor {
                    epoch: EpochNumber::try_from(1)?,
                    position: FeedPosition::ZERO,
                },
                origin: origin.clone(),
            }),
            posted_as_role: None,
        };
        positions.push(write.append_record(Record::Post(post)).await?);
        write.commit().await?;
        ids.push(post_id);
    }
    assert!(positions[0].position < positions[1].position);
    let request = json!({"scope":{"kind":"allWorkspaces"},"selection":{"kind":"afterPosition","after":{"kind":"aggregate","cursor":{"perProject":{}}}},"page":{"limit":1,"cursor":null}});
    let mut next_request = request;
    // Heads choose B1, A1, A2, B2. A2 cannot jump ahead of A1 despite its earlier clock.
    for (index, expected) in [2, 0, 1, 3].into_iter().enumerate() {
        let page = api.call("post_list", next_request.clone()).await?;
        assert_eq!(
            page.pointer("/result/page/records/0/postId")
                .and_then(Value::as_str),
            Some(ids[expected].as_str()),
            "head merge page {index}: {page}"
        );
        if index < 3 {
            let cursor = page
                .pointer("/result/page/nextCursor")
                .and_then(Value::as_str)
                .ok_or("continuation missing")?;
            next_request["page"]["cursor"] = json!(cursor);
        } else {
            assert_eq!(
                page.pointer("/result/page/nextCursor"),
                Some(&Value::Null),
                "last page has no continuation: {page}"
            );
        }
    }
    api.stop().await?;
    fleet_runtime.shutdown().await?;
    Ok(())
}
fn assert_accepted(value: &Value) {
    assert_eq!(
        value
            .pointer("/result/result/decision/kind")
            .and_then(Value::as_str),
        Some("accepted"),
        "{value}"
    );
}
