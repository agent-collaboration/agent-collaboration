#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Fleet metadata uses the public MCP/client path and a real central/session/cache.
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::{
    CollaborationApplication, MachineIdentity, ProjectAdmission, ProjectAdmissionProps,
    ServiceIdentity,
};
use message_board::*;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn adr_lifecycle_is_replayable_through_mcp() -> TestResult {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("fleet MCP proof".to_owned())?,
    };
    let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
        directory.path(),
        Arc::new(board_fixture::PendingRegistration),
        &origin,
    )
    .await?;
    let fleet = Arc::new(fleet);
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: board_fixture::new_registry(directory.path())?,
        fleet,
        identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(
                "00000000-0000-4000-8000-000000000001".to_owned(),
            )?,
            Some("fleet MCP proof"),
        )?,
    })?);
    let api = ServedCollaborationApi::start(
        directory.path(),
        CollaborationApplication::new(
            ServiceIdentity::new(
                "00000000-0000-4000-8000-000000000001",
                "00000000-0000-4000-8000-000000000002",
            )?
            .with_project_admission(admission),
        ),
    )
    .await?;
    let workspace = WorkspaceId::generate();
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"workspaceId":workspace,"actor":{"kind":"human","humanId":"owner"},"actingFor":null});
    let mut create = header();
    create["name"] = json!("fleet");
    create["description"] = json!("");
    assert_accepted(&api.call("workspace_create", create).await?);
    let adr = AdrId::generate();
    let mut create = header();
    create["adrId"] = json!(adr);
    create["title"] = json!("One store per project");
    create["text"] = json!("# Decision\nKeep project history together.");
    create["supersedes"] = Value::Null;
    create["madeIn"] = Value::Null;
    create["repositoryFiles"] = json!([]);
    let original = api.call("adr_create", create.clone()).await?;
    assert_accepted(&original);
    assert_eq!(api.call("adr_create", create).await?, original);
    let mut transition = header();
    transition["adrId"] = json!(adr);
    transition["transition"] = json!({"kind":"accept"});
    transition["expectedRevision"] = json!(0);
    assert_accepted(&api.call("adr_transition", transition.clone()).await?);
    transition["requestId"] = json!(RequestId::generate());
    let stale = api.call("adr_transition", transition).await?;
    assert_eq!(
        stale.pointer("/result/result/decision/reasons/0/kind"),
        Some(&json!("staleRevision")),
        "{stale}"
    );
    let shown = api
        .call("adr_show", json!({"record":{"kind":"adr","id":adr}}))
        .await?;
    assert_eq!(
        shown.pointer("/result/record/status"),
        Some(&json!("accepted")),
        "{shown}"
    );
    assert_eq!(
        shown.pointer("/result/record/text"),
        Some(&json!("# Decision\nKeep project history together."))
    );
    let listed = api
        .call(
            "adr_list",
            json!({"workspaceId":workspace,"page":{"limit":1,"cursor":null}}),
        )
        .await?;
    assert_eq!(
        listed.pointer("/result/records/0/record/adrId"),
        Some(&json!(adr)),
        "{listed}"
    );
    api.stop().await?;
    fleet_runtime.shutdown().await?;
    Ok(())
}
fn assert_accepted(value: &Value) {
    assert_eq!(
        value.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{value}"
    );
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn a_thread_can_discuss_an_immutable_adr_in_its_workspace() -> TestResult {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let registry = board_fixture::new_registry(directory.path())?;
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("ADR subject proof".to_owned())?,
    };
    let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
        directory.path(),
        Arc::new(board_fixture::PendingRegistration),
        &origin,
    )
    .await?;
    let fleet = Arc::new(fleet);
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: fleet.clone(),
        identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(
                "00000000-0000-4000-8000-000000000001".to_owned(),
            )?,
            Some("ADR subject proof"),
        )?,
    })?);
    let api = ServedCollaborationApi::start(
        directory.path(),
        CollaborationApplication::new(
            ServiceIdentity::new(
                "00000000-0000-4000-8000-000000000001",
                "00000000-0000-4000-8000-000000000002",
            )?
            .with_project_admission(admission),
        ),
    )
    .await?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let adr = AdrId::generate();
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,"actor":{"kind":"human","humanId":"owner"}});
    let mut create = header();
    create["workspaceId"] = json!(workspace);
    create["name"] = json!("workspace");
    create["description"] = json!("");
    assert_accepted(&api.call("workspace_create", create).await?);
    let mut create = header();
    create["workspaceId"] = json!(workspace);
    create["boardId"] = json!(board);
    create["name"] = json!("board");
    create["description"] = json!("");
    create["kind"] = json!({"kind":"area"});
    assert_accepted(&api.call("board_create", create).await?);
    let mut create = header();
    create["boardId"] = json!(board);
    create["projectId"] = json!(project);
    create["title"] = json!("project");
    board_fixture::prepare_registered_creation(
        &fleet,
        &registry,
        &serde_json::from_value(create.clone())?,
        &origin,
        &runtime_primitives::SystemClock,
    )
    .await?;
    assert_accepted(&api.call("project_create", create).await?);
    let mut create = header();
    create["workspaceId"] = json!(workspace);
    create["adrId"] = json!(adr);
    create["title"] = json!("Decision");
    create["text"] = json!("Immutable decision content");
    create["supersedes"] = Value::Null;
    create["madeIn"] = Value::Null;
    create["repositoryFiles"] = json!([]);
    assert_accepted(&api.call("adr_create", create).await?);
    let post = PostId::generate();
    let mut create = header();
    create["projectId"] = json!(project);
    create["postId"] = json!(post);
    create["about"] = json!({"kind":"workspaceAdr","subject":adr});
    create["text"] = json!("Discuss the decision");
    create["references"] = json!([]);
    create["role"] = Value::Null;
    create["watch"] = json!(false);
    create["addressees"] = json!([]);
    assert_accepted(&api.call("thread_create", create).await?);
    let shown = api
        .call(
            "thread_show",
            json!({"thread":{"thread":post},"reader":{"kind":"anonymous"}}),
        )
        .await?;
    assert_eq!(
        shown.pointer("/result/about"),
        Some(&json!({"kind":"workspaceAdr","subject":adr})),
        "{shown}"
    );
    api.stop().await?;
    fleet_runtime.shutdown().await?;
    Ok(())
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn project_show_reads_the_existing_fleet_directory_record() -> TestResult {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let shown = fixture
        .api
        .call("project_show", json!({"projectId":project}))
        .await?;
    assert_eq!(
        shown.pointer("/result/project/id"),
        Some(&json!(project)),
        "{shown}"
    );
    assert_eq!(
        shown.pointer("/result/project/board"),
        Some(&json!(fixture.board)),
        "{shown}"
    );
    fixture.stop().await?;
    Ok(())
}
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::BoardFixture;
