//! V1/V2 enter through the real stateless MCP socket and native project stores.
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
use board_fixture::fixture_peer;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::{
    ApplicationFixture, MachineIdentity, ProjectAdmission, ProjectAdmissionProps,
    ProjectStoreRegistry,
};
use message_board::*;
use serde_json::{Value, json};
use std::sync::Arc;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn native_mcp_admission_replays_decisions_and_isolates_project_authority() -> TestResult {
    let directory = tempfile::tempdir()?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("MCP proof".to_owned())?,
    };
    let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
        directory.path(),
        Arc::new(board_fixture::PendingRegistration),
        &origin,
    )
    .await?;
    let fleet = Arc::new(fleet);
    let registry = Arc::new(ProjectStoreRegistry::new(
        directory.path(),
        Some(fixture_peer()?),
        Arc::new(runtime_primitives::SystemClock),
    ));
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: fleet.clone(),
        identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(
                "00000000-0000-4000-8000-000000000001".to_owned(),
            )?,
            Some("MCP proof"),
        )?,
    })?);
    let identity = ApplicationFixture::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission);
    let api = ServedCollaborationApi::start(directory.path(), (identity).build())?;
    let actor = json!({"kind":"human","humanId":"owner"});
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let workstream = WorkstreamId::generate();
    let task = TaskId::generate();
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,"actor":actor});
    let mut create = header();
    create["workspaceId"] = json!(workspace);
    create["name"] = json!("workspace");
    create["description"] = json!("");
    accepted(&api.call("workspace_create", create).await?)?;
    let mut create = header();
    create["workspaceId"] = json!(workspace);
    create["boardId"] = json!(board);
    create["name"] = json!("board");
    create["description"] = json!("");
    create["kind"] = json!({"kind":"area"});
    accepted(&api.call("board_create", create).await?)?;
    let mut create = header();
    create["projectId"] = json!(project);
    create["boardId"] = json!(board);
    create["title"] = json!("project");
    board_fixture::prepare_registered_creation(
        &fleet,
        &registry,
        &serde_json::from_value(create.clone())?,
        &origin,
        &runtime_primitives::SystemClock,
    )
    .await?;
    accepted(&api.call("project_create", create).await?)?;
    let project_request = |operation: Value| {
        let mut request = header();
        request["projectId"] = json!(project);
        request["operation"] = operation;
        request
    };
    let denied = project_request(
        json!({"kind":"workstreamCreate","workstreamId":workstream,"name":"work","goal":"done"}),
    );
    let rejected = api.call("workstream_create", denied.clone()).await?;
    assert_eq!(
        rejected
            .pointer("/result/result/decision/kind")
            .and_then(Value::as_str),
        Some("rejected")
    );
    assert_eq!(api.call("workstream_create", denied).await?, rejected);
    accepted(&api.call("role_grant",project_request(json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?)?;
    accepted(&api.call("workstream_create",project_request(json!({"kind":"workstreamCreate","workstreamId":workstream,"name":"work","goal":"done"}))).await?)?;
    let create = project_request(
        json!({"kind":"taskCreate","taskId":task,"workstreamId":workstream,"title":"task","body":"","blockedBy":[],"relatedTo":[]}),
    );
    let first = api.call("task_create", create.clone()).await?;
    accepted(&first)?;
    assert_eq!(api.call("task_create", create.clone()).await?, first);
    let mut conflict = create;
    conflict["operation"]["title"] = json!("changed");
    assert_eq!(
        api.call("task_create", conflict)
            .await?
            .pointer("/result/result/kind")
            .and_then(Value::as_str),
        Some("conflictingRequest")
    );
    let shown = api
        .call(
            "task_show",
            json!({"projectId":project,"record":{"kind":"task","id":task}}),
        )
        .await?;
    assert_eq!(
        shown
            .pointer("/result/record/title")
            .and_then(Value::as_str),
        Some("task")
    );
    let target = TaskId::generate();
    accepted(&api.call("task_create",project_request(json!({"kind":"taskCreate","taskId":target,"workstreamId":workstream,"title":"target","body":"","blockedBy":[],"relatedTo":[]}))).await?)?;
    let duplicate = project_request(
        json!({"kind":"taskAction","taskId":task,"action":{"kind":"duplicate","of":target,"note":"same work"},"expectedRevision":0}),
    );
    let decision = api.call("task_duplicate", duplicate.clone()).await?;
    accepted(&decision)?;
    assert_eq!(api.call("task_duplicate", duplicate).await?, decision);
    assert_eq!(
        api.call(
            "task_show",
            json!({"projectId":project,"record":{"kind":"task","id":task}})
        )
        .await?
        .pointer("/result/record/status")
        .and_then(Value::as_str),
        Some("dropped")
    );
    let trail = api.call("work_trail_show",json!({"projectId":project,"selection":{"kind":"forward","after":{"epoch":1,"position":0}},"limit":100,"subject":{"kind":"task","subject":task},"tags":[],"includeRedacted":false,"reader":{"kind":"anonymous"}})).await?;
    assert_eq!(
        trail
            .pointer("/result/value/page/rows/0/text")
            .and_then(Value::as_str),
        Some(format!("duplicate of {}: same work", target.as_str()).as_str()),
        "{trail}"
    );
    let root_id = PostId::try_from(workstream.as_str().to_owned())?;
    let mut thread_request = header();
    thread_request["postId"] = json!(root_id);
    thread_request["projectId"] = json!(project);
    thread_request["about"] = json!({"kind":"workstream","subject":workstream});
    thread_request["text"] = json!("separate thread namespace");
    thread_request["references"] = json!([]);
    thread_request["role"] = Value::Null;
    thread_request["watch"] = json!(false);
    thread_request["addressees"] =
        json!([{"kind":"role","recipient":{"kind":"verifier","subject":workstream}}]);
    let thread_reply = api.call("thread_create", thread_request).await?;
    accepted(&thread_reply)?;
    assert!(
        !thread_reply
            .pointer("/result/reader/unread")
            .and_then(Value::as_array)
            .ok_or("thread reader missing")?
            .iter()
            .any(|record| record.get("kind") == Some(&json!("task"))),
        "thread/workstream UUID collision must not mix unread subjects: {thread_reply}"
    );
    let intents = api
        .call("intent_list", json!({"projectId":project,"page":{}}))
        .await?;
    assert!(
        intents
            .pointer("/result/records")
            .and_then(Value::as_array)
            .ok_or("intents missing")?
            .iter()
            .any(
                |record| record.pointer("/record/cause/id").and_then(Value::as_str)
                    == Some(root_id.as_str())
                    && record.pointer("/record/state/reason/kind") == Some(&json!("noHolder"))
            ),
        "{intents}"
    );
    let initialize = json!({"protocolVersion":1,"actingFor":null,"reader":actor,"scope":{"kind":"project","projectId":project},"readMode":"unread","initializations":[{"projectId":project,"requestId":RequestId::generate()}],"page":{"limit":100,"cursor":null}});
    let initialized = api.call("inbox_fetch", initialize.clone()).await?;
    assert_eq!(
        initialized.pointer("/result/page/initializations/0/outcome/reply/result/decision/change"),
        Some(&json!("changed")),
        "{initialized}"
    );
    let original_mark = initialized
        .pointer("/result/page/initializations/0/outcome/reply/reader/readThrough")
        .ok_or("initialization mark missing")?
        .clone();
    let later = TaskId::generate();
    accepted(&api.call("task_create",project_request(json!({"kind":"taskCreate","taskId":later,"workstreamId":workstream,"title":"later","body":"","blockedBy":[],"relatedTo":[]}))).await?)?;
    let retried = api.call("inbox_fetch", initialize).await?;
    assert_eq!(
        retried.pointer("/result/page/initializations/0/outcome/reply/result"),
        initialized.pointer("/result/page/initializations/0/outcome/reply/result")
    );
    assert_eq!(
        retried.pointer("/result/page/initializations/0/outcome/reply/reader/readThrough"),
        Some(&original_mark)
    );
    assert_ne!(
        retried.pointer("/result/page/initializations/0/outcome/reply/reader/unreadCount"),
        initialized.pointer("/result/page/initializations/0/outcome/reply/reader/unreadCount")
    );
    let late_project = ProjectId::generate();
    let late_workstream = WorkstreamId::generate();
    let mut early_request = header();
    early_request["projectId"] = json!(late_project);
    early_request["operation"] = json!({"kind":"workstreamCreate","workstreamId":late_workstream,"name":"early","goal":"wait for project"});
    let missing_scope = api.call("workstream_create", early_request.clone()).await?;
    assert_eq!(
        missing_scope.pointer("/result/result/kind"),
        Some(&json!("scopeUnavailable")),
        "{missing_scope}"
    );
    let mut create_late = header();
    create_late["projectId"] = json!(late_project);
    create_late["boardId"] = json!(board);
    create_late["title"] = json!("late project");
    board_fixture::prepare_registered_creation(
        &fleet,
        &registry,
        &serde_json::from_value(create_late.clone())?,
        &origin,
        &runtime_primitives::SystemClock,
    )
    .await?;
    accepted(&api.call("project_create", create_late).await?)?;
    let mut grant_late = header();
    grant_late["projectId"] = json!(late_project);
    grant_late["operation"] = json!({"kind":"roleGrant","subject":{"kind":"project","subject":late_project},"role":"lead","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null});
    accepted(&api.call("role_grant", grant_late).await?)?;
    let retried_missing_scope = api.call("workstream_create", early_request).await?;
    accepted(&retried_missing_scope)?;
    assert!(registry.existing_project(&project).await?.is_some());
    api.stop().await?;
    fleet_runtime.shutdown().await?;
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
fn accepted(value: &Value) -> TestResult {
    assert_eq!(
        value
            .pointer("/result/result/decision/kind")
            .and_then(Value::as_str),
        Some("accepted"),
        "{value}"
    );
    Ok(())
}
