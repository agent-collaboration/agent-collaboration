//! V8 uses the executable, v3 manifest, MCP socket, admission and native stores.
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "This fixture source belongs to the split lane, which fixes its allowances at integration."
)]
#[path = "../../collaboration-service/tests/support/native_registry.rs"]
mod native_registry;
use collaboration_contracts::MachineIdentity;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::ApplicationFixture;

use collaboration_service::ProjectAdmission;
use collaboration_service::ProjectAdmissionProps;
use message_board::*;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};
type TestError = Box<dyn std::error::Error + Send + Sync>;
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
use board_cli_execution::{accepted, cli};
fn request(project: &ProjectId, operation: Value) -> Value {
    json!({"requestId":RequestId::generate(),"projectId":project,"actor":{"kind":"human","humanId":"owner"},"actingFor":null,"protocolVersion":1,"operation":operation})
}
fn request_file(directory: &Path, name: &str, value: &Value) -> Result<String, TestError> {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value)?)?;
    Ok(path.to_str().ok_or("non-UTF8 fixture path")?.to_owned())
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn renamed_command_groups_execute_real_board_journey_and_keep_caller_request_ids()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let registry = native_registry::new_registry(directory.path())?;
    let origin = message_board::ActorOrigin {
        installation: message_board::ServiceId::try_from(
            "00000000-0000-4000-8000-000000000001".to_owned(),
        )?,
        machine: message_board::MachineLabel::try_from("CLI native proof".to_owned())?,
    };
    let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
        directory.path(),
        Arc::new(native_registry::PendingRegistration),
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
            Some("CLI native proof"),
        )?,
    })?);
    let automation = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await?,
    ));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await?,
    );
    let routes = Arc::new(collaboration_runtime::SessionDeliveryRouter::new(Vec::new()));
    let delivery = collaboration_service::SubscriptionDeliveryService::new(
        collaboration_service::SubscriptionDeliveryServiceProps {
            board_availability: collaboration_service::BoardAvailability::Available(Arc::new(
                tokio::sync::Mutex::new(admission.subscription_window_store(interactions.clone())),
            )),
            push_store: interactions.clone(),
            delivery: routes.clone(),
            presence: routes.clone(),
            machine_identity: MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                Some("CLI native proof"),
            )?,
            clock: Arc::new(collaboration_service::SystemSubscriptionClock),
        },
    );
    delivery.start().await?;
    let api = ServedCollaborationApi::start(
        directory.path(),
        (ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission)
        .with_automation_store(automation)
        .with_subscription_delivery_service(delivery.clone(), routes))
        .build(),
    )?;
    let actor = r#"{"kind":"human","humanId":"owner"}"#;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let workstream = WorkstreamId::generate();
    let task = TaskId::generate();
    let subtask = SubtaskId::generate();
    accepted(
        &cli(
            directory.path(),
            &[
                "workspace",
                "create",
                "--workspace-id",
                workspace.as_str(),
                "--name",
                "CLI workspace",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    accepted(
        &cli(
            directory.path(),
            &[
                "board",
                "create",
                "--board-id",
                board.as_str(),
                "--workspace-id",
                workspace.as_str(),
                "--name",
                "CLI board",
                "--kind",
                r#"{"kind":"initiative","goal":"real path","doneWhen":"proof passes"}"#,
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let creation = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
        board_id: board.clone(),
        title: ResourceName::try_from("CLI project".to_owned())?,
        description: None,
        actor: serde_json::from_str(actor)?,
        acting_for: None,
    };
    native_registry::prepare_registered_creation(
        &fleet,
        &registry,
        &creation,
        &ActorOrigin {
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("CLI native proof".to_owned())?,
        },
    )
    .await?;
    accepted(
        &cli(
            directory.path(),
            &[
                "project",
                "create",
                "--request-id",
                creation.request_id.as_str(),
                "--project-id",
                project.as_str(),
                "--board-id",
                board.as_str(),
                "--title",
                "CLI project",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let shown = cli(
        directory.path(),
        &["project", "show", "--project-id", project.as_str()],
    )
    .await?;
    assert_eq!(
        shown.pointer("/result/record/project/id"),
        Some(&json!(project)),
        "{shown}"
    );
    assert_eq!(
        shown.pointer("/result/record/project/title"),
        Some(&json!("CLI project")),
        "{shown}"
    );
    let grant = request_file(
        directory.path(),
        "grant.json",
        &request(
            &project,
            json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":{"kind":"human","humanId":"owner"},"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
        ),
    )?;
    accepted(
        &cli(
            directory.path(),
            &["role", "grant", "--request-file", &grant],
        )
        .await?,
    );
    accepted(
        &cli(
            directory.path(),
            &[
                "workstream",
                "create",
                "--project-id",
                project.as_str(),
                "--workstream-id",
                workstream.as_str(),
                "--name",
                "implementation",
                "--goal",
                "exercise commands",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let request_id = RequestId::generate();
    let task_args = [
        "task",
        "create",
        "--project-id",
        project.as_str(),
        "--workstream-id",
        workstream.as_str(),
        "--task-id",
        task.as_str(),
        "--title",
        "CLI task",
        "--request-id",
        request_id.as_str(),
        "--actor",
        actor,
    ];
    let first = cli(directory.path(), &task_args).await?;
    accepted(&first);
    assert_eq!(cli(directory.path(), &task_args).await?, first);
    let target = TaskId::generate();
    accepted(
        &cli(
            directory.path(),
            &[
                "task",
                "create",
                "--project-id",
                project.as_str(),
                "--workstream-id",
                workstream.as_str(),
                "--task-id",
                target.as_str(),
                "--title",
                "target",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    accepted(
        &cli(
            directory.path(),
            &[
                "task",
                "link",
                "--project-id",
                project.as_str(),
                "--from",
                task.as_str(),
                "--to",
                target.as_str(),
                "--kind",
                "blockedBy",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    accepted(
        &cli(
            directory.path(),
            &[
                "subtask",
                "add",
                "--project-id",
                project.as_str(),
                "--task-id",
                task.as_str(),
                "--subtask-id",
                subtask.as_str(),
                "--text",
                "CLI checklist",
                "--place",
                "80",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    accepted(
        &cli(
            directory.path(),
            &[
                "work-trail",
                "add",
                "Decision from CLI",
                "--project-id",
                project.as_str(),
                "--tag",
                "decision",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let decisions = cli(
        directory.path(),
        &[
            "work-trail",
            "show",
            "--project-id",
            project.as_str(),
            "--decisions",
        ],
    )
    .await?;
    assert_eq!(
        decisions.pointer("/result/record/value/page/rows/0/text"),
        Some(&json!("Decision from CLI")),
        "{decisions}"
    );
    let root = PostId::generate();
    let text_file = directory.path().join("thread.md");
    std::fs::write(&text_file, "Discuss the task")?;
    let about = format!("task:{}", task.as_str());
    accepted(
        &cli(
            directory.path(),
            &[
                "thread",
                "create",
                "--project-id",
                project.as_str(),
                "--post-id",
                root.as_str(),
                "--text-file",
                text_file.to_str().ok_or("text path")?,
                "--about",
                &about,
                "--actor",
                actor,
                "--no-watch",
            ],
        )
        .await?,
    );
    accepted(
        &cli(
            directory.path(),
            &[
                "post",
                "post",
                "--placement",
                "thread",
                "--thread-id",
                root.as_str(),
                "--text",
                "Reply",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"workspaceId":workspace,"actor":{"kind":"human","humanId":"owner"},"actingFor":null});
    let fact = FactId::generate();
    let proof_kind = FactKind::EvaluatorResult {
        guard: "CLI proof".to_owned(),
        subject: project.clone().into(),
        outcome: GuardOutcome::Met,
        evidence: Vec::new(),
        evaluator: EvaluatorRef {
            actor: RecordedActor::declared(serde_json::from_str(actor)?),
            engine: None,
            calibration: None,
        },
        executor: None,
    };
    let fact_file = request_file(
        directory.path(),
        "fact.json",
        &request(
            &project,
            json!({"kind":"factRecord","fact":{"factId":fact,"kind":proof_kind,"links":[{"kind":"project","id":project}],"producerTime":"2026-10-12T00:00:00Z","anchor":null}}),
        ),
    )?;
    accepted(
        &cli(
            directory.path(),
            &["fact", "record", "--request-file", &fact_file],
        )
        .await?,
    );
    let adr = AdrId::generate();
    let mut adr_request = header();
    adr_request["adrId"] = json!(adr);
    adr_request["title"] = json!("CLI ADR");
    adr_request["text"] = json!("# Decision\nNative stores.");
    adr_request["supersedes"] = Value::Null;
    adr_request["madeIn"] = Value::Null;
    adr_request["repositoryFiles"] = json!([]);
    let adr_file = request_file(directory.path(), "adr.json", &adr_request)?;
    accepted(
        &cli(
            directory.path(),
            &["adr", "create", "--request-file", &adr_file],
        )
        .await?,
    );
    let feed = cli(
        directory.path(),
        &[
            "feed",
            "read",
            "--project-id",
            project.as_str(),
            "--after",
            r#"{"epoch":1,"position":0}"#,
            "--limit",
            "100",
        ],
    )
    .await?;
    assert!(
        feed.pointer("/result/record/value/entries")
            .and_then(Value::as_array)
            .ok_or("feed entries")?
            .iter()
            .any(|entry| entry.pointer("/content/record/record/text")
                == Some(&json!("Decision from CLI"))),
        "{feed}"
    );
    accepted(
        &cli(
            directory.path(),
            &[
                "thread",
                "subscribe",
                "--project-id",
                project.as_str(),
                "--actor",
                actor,
                "--mode",
                "poll",
                "--quiet",
                "0s",
                "--cap",
                "0s",
            ],
        )
        .await?,
    );
    let waited = cli(
        directory.path(),
        &[
            "thread",
            "wait",
            "--project-id",
            project.as_str(),
            "--actor",
            actor,
            "--max-wait",
            "0s",
        ],
    )
    .await?;
    assert_eq!(
        waited.pointer("/result/record/outcome/kind"),
        Some(&json!("timedOut")),
        "{waited}"
    );
    assert_eq!(
        waited.pointer("/result/record/renewals/0/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{waited}"
    );
    let renewal_id: RequestId = serde_json::from_value(
        waited
            .pointer("/result/record/renewals/0/requestId")
            .ok_or("renewal request ID")?
            .clone(),
    )?;
    let store = registry
        .existing_project(&project)
        .await?
        .ok_or("native project missing")?;
    let mut read = store.snapshot().await?;
    let links = read.task_links().await?;
    assert_eq!(
        links,
        vec![TaskLink {
            from: task.clone(),
            kind: TaskLinkKind::BlockedBy,
            to: target
        }]
    );
    assert!(read.record(&request_id.into()).await?.is_some());
    assert!(read.record(&renewal_id.into()).await?.is_some());
    read.finish().await?;
    delivery.shutdown().await;
    api.stop().await?;
    fleet_runtime.shutdown().await?;
    Ok(())
}
