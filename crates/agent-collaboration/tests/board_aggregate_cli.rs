//! Aggregate histories, unread continuation and independent acknowledgements through the executable.
#[path = "../../collaboration-service/tests/support/native_authority.rs"]
mod native_authority;

#[path = "../../collaboration-service/tests/support/native_registry.rs"]
mod native_registry;
use collaboration_contracts::MachineIdentity;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::ApplicationFixture;

use collaboration_service::ProjectAdmission;
use collaboration_service::ProjectAdmissionProps;
use message_board::*;
use runtime_primitives::{Clock, SyncTime, SystemClock};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
use board_cli_execution::{accepted, cli};
type TestError = Box<dyn std::error::Error + Send + Sync>;
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn aggregate_cli_pages_and_batch_acknowledgements_keep_project_keys() -> Result<(), TestError>
{
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let registry = native_registry::new_registry(directory.path())?;
    let origin = message_board::ActorOrigin {
        installation: message_board::ServiceId::try_from(
            "00000000-0000-4000-8000-000000000001".to_owned(),
        )?,
        machine: message_board::MachineLabel::try_from("aggregate CLI proof".to_owned())?,
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
            Some("aggregate CLI proof"),
        )?,
    })?);
    let api = ServedCollaborationApi::start(
        directory.path(),
        (ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone()))
        .build(),
    )?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let projects = [ProjectId::generate(), ProjectId::generate()];
    let actor = r#"{"kind":"human","humanId":"owner"}"#;
    let reader = r#"{"kind":"human","humanId":"reader"}"#;
    accepted(
        &cli(
            directory.path(),
            &[
                "workspace",
                "create",
                "--workspace-id",
                workspace.as_str(),
                "--name",
                "aggregate",
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
                "board",
                "--kind",
                r#"{"kind":"area"}"#,
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    for (index, project) in projects.iter().enumerate() {
        let input = ProjectCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: project.clone(),
            board_id: board.clone(),
            title: ResourceName::try_from(format!("project {index}"))?,
            description: None,
            actor: serde_json::from_str(actor)?,
            acting_for: None,
        };
        native_registry::prepare_registered_creation(
            &fleet,
            &registry,
            &input,
            &ActorOrigin {
                installation: ServiceId::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                machine: MachineLabel::try_from("aggregate CLI proof".to_owned())?,
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
                    input.request_id.as_str(),
                    "--project-id",
                    project.as_str(),
                    "--board-id",
                    board.as_str(),
                    "--title",
                    &format!("project {index}"),
                    "--actor",
                    actor,
                ],
            )
            .await?,
        );
    }
    for project in &projects {
        let request = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":{"kind":"human","humanId":"owner"},"actingFor":null,"projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":{"kind":"human","humanId":"owner"},"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}});
        let grant = api.call("role_grant", request).await?;
        assert_eq!(
            grant.pointer("/result/result/decision/kind"),
            Some(&json!("accepted")),
            "{grant}"
        );
    }
    let root_file = directory.path().join("aggregate-root.md");
    std::fs::write(&root_file, "root")?;
    let initialization: Vec<_> = projects
        .iter()
        .map(|project| json!({"projectId":project,"requestId":RequestId::generate()}).to_string())
        .collect();
    let first_init = initialization.first().ok_or("entry")?;
    let second_init = initialization.get(1).ok_or("entry")?;
    let initialized = cli(
        directory.path(),
        &[
            "inbox",
            "fetch",
            "--scope",
            "board",
            "--board-id",
            board.as_str(),
            "--reader",
            reader,
            "--initialization",
            first_init,
            "--initialization",
            second_init,
        ],
    )
    .await?;
    assert_eq!(
        initialized
            .pointer("/result/page/initializations")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2),
        "{initialized}"
    );
    let roots = [PostId::generate(), PostId::generate()];
    let mut expected = Vec::new();
    for (project, root) in projects.iter().zip(&roots) {
        accepted(
            &cli(
                directory.path(),
                &[
                    "thread",
                    "create",
                    "--post-id",
                    root.as_str(),
                    "--project-id",
                    project.as_str(),
                    "--about",
                    "project:Aggregate root",
                    "--no-watch",
                    "--text-file",
                    root_file.to_str().ok_or("root path")?,
                    "--actor",
                    actor,
                ],
            )
            .await?,
        );
        expected.push(root.clone());
        accepted(
            &cli(
                directory.path(),
                &[
                    "thread",
                    "watch",
                    "--thread-id",
                    root.as_str(),
                    "--actor",
                    reader,
                ],
            )
            .await?,
        );
        let reply = PostId::generate();
        accepted(
            &cli(
                directory.path(),
                &[
                    "post",
                    "post",
                    "--post-id",
                    reply.as_str(),
                    "--placement",
                    "thread",
                    "--thread-id",
                    root.as_str(),
                    "--text",
                    "reply",
                    "--actor",
                    actor,
                ],
            )
            .await?,
        );
        expected.push(reply);
    }
    let mut cursor: Option<String> = None;
    let mut unread = Vec::new();
    loop {
        let mut args = vec![
            "inbox",
            "fetch",
            "--scope",
            "board",
            "--board-id",
            board.as_str(),
            "--reader",
            reader,
            "--limit",
            "1",
        ];
        if let Some(token) = &cursor {
            args.extend(["--cursor", token.as_str()]);
        }
        let response = cli(directory.path(), &args).await?;
        unread.extend(
            response
                .pointer("/result/page/records")
                .and_then(Value::as_array)
                .ok_or("inbox records")?
                .iter()
                .filter_map(|row| row.pointer("/post/postId").cloned()),
        );
        cursor = response
            .pointer("/result/page/nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    let mut observed_ids: Vec<_> = unread
        .into_iter()
        .map(serde_json::from_value::<PostId>)
        .collect::<Result<_, _>>()?;
    observed_ids.sort();
    let mut sorted = expected.clone();
    sorted.sort();
    assert_eq!(observed_ids, sorted);
    let after = r#"{"kind":"aggregate","cursor":{"perProject":{}}}"#;
    let mut cursor: Option<String> = None;
    let mut posts = Vec::new();
    loop {
        let mut args = vec![
            "post",
            "list",
            "--scope",
            "workspace",
            "--workspace-id",
            workspace.as_str(),
            "--selection",
            "after-position",
            "--after",
            after,
            "--limit",
            "1",
        ];
        if let Some(token) = &cursor {
            args.extend(["--cursor", token.as_str()]);
        }
        let response = cli(directory.path(), &args).await?;
        posts.extend(
            response
                .pointer("/result/page/records")
                .and_then(Value::as_array)
                .ok_or("post records")?
                .iter()
                .filter_map(|post| post.get("postId").cloned()),
        );
        cursor = response
            .pointer("/result/page/nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    let mut posts: Vec<PostId> = posts
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?;
    posts.sort();
    assert_eq!(posts, sorted);
    let mut entries = Vec::new();
    for project in &projects {
        let store = registry.existing_project(project).await?.ok_or("store")?;
        let read = store.snapshot().await?;
        let through = read.high_water();
        read.finish().await?;
        entries.push(json!({"requestId":RequestId::generate(),"scope":{"kind":"project","projectId":project},"through":through}));
    }
    let expired_at = SyncTime::from_unix_millis(SystemClock.now().unix_millis() - 120_000);
    let lease = fleet_sync_protocol::LeaseStatus {
        database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(
            projects[1].as_str(),
        )?)?,
        epoch: fleet_sync_protocol::Epoch::new(1)?,
        holder: native_registry::fixture_peer()?,
        state: fleet_sync_protocol::LeaseState::Held,
        expires_at: SyncTime::from_unix_millis(expired_at.unix_millis() + 60_000),
        remaining_ms: 60_000,
    };
    registry
        .database(&projects[1])
        .await?
        .observe_renewal(
            fleet_sync_protocol::RenewAnswer::Renewed {
                recorded: fleet_sync_protocol::RenewResult {
                    epoch: lease.epoch,
                    expires_at: lease.expires_at,
                },
                current: lease.clone(),
                transfer_in_progress: None,
                replayed: false,
            },
            expired_at,
        )
        .await?;
    let strings: Vec<_> = entries.iter().map(Value::to_string).collect();
    let first_entry = strings.first().ok_or("entry")?;
    let second_entry = strings.get(1).ok_or("entry")?;
    let args = [
        "inbox",
        "acknowledge",
        "--actor",
        reader,
        "--entry",
        first_entry,
        "--entry",
        second_entry,
    ];
    let partial = cli(directory.path(), &args).await?;
    assert_eq!(
        partial.pointer("/result/record/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{partial}"
    );
    assert_eq!(
        partial.pointer("/result/record/outcomes/1/outcome/reply/result/kind"),
        Some(&json!("leaseExpired")),
        "{partial}"
    );
    native_authority::observe_same_epoch_renewal(&registry, &projects[1], lease, SystemClock.now())
        .await?;
    let retried = cli(directory.path(), &args).await?;
    assert_eq!(
        retried.pointer("/result/record/outcomes/0/outcome/reply/result"),
        partial.pointer("/result/record/outcomes/0/outcome/reply/result")
    );
    assert_eq!(
        retried.pointer("/result/record/outcomes/1/outcome/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{retried}"
    );
    for (project, entry) in projects.iter().zip(entries) {
        let id: RequestId = serde_json::from_value(entry["requestId"].clone())?;
        let store = registry.existing_project(project).await?.ok_or("store")?;
        let mut read = store.snapshot().await?;
        assert!(read.record(&id.into()).await?.is_some());
        read.finish().await?;
    }
    api.stop().await?;
    fleet_runtime.shutdown().await?;
    Ok(())
}
