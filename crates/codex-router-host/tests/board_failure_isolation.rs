use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{BoardFailureKind, PageLimit, PageRequest, WorkspaceListRequest};
use std::os::unix::fs::DirBuilderExt;

#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
#[allow(dead_code)]
// This Host restart proof uses the shared board setup, not every fixture journey.
mod board_fixture;

#[tokio::test]
async fn board_open_failure_keeps_unrelated_control_methods_available()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(format!(
        "/tmp/host-board-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    std::fs::create_dir(root.join("fleet.sqlite"))?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("absent-native.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: Some(root.join("test-peer-registry")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(&root, "board-failure-isolation", "1").await?;

    let _endpoints = client.list_endpoints().await?;
    let board_result = client
        .workspace_list(WorkspaceListRequest {
            repository: None,
            page: PageRequest {
                limit: PageLimit::try_from(10)?,
                cursor: None,
            },
        })
        .await;
    runtime.shutdown().await?;
    if root.join("provider-operations.sqlite-wal").exists()
        || root.join("provider-operations.sqlite-shm").exists()
    {
        return Err("provider operation sidecars outlived runtime shutdown".into());
    }
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    std::fs::remove_dir(root)?;
    if !matches!(
        board_result,
        Err(BoardClientError::Rejected(error)) if error.kind == BoardFailureKind::BoardUnavailable
    ) {
        return Err(
            "unavailable board store did not return the typed boardUnavailable error".into(),
        );
    }
    Ok(())
}

#[tokio::test]
async fn corrupt_project_at_startup_keeps_runtime_and_endpoints_available()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    std::fs::set_permissions(root, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let fleet = collaboration_service::FleetClient::open_local_stand_in(root).await?;
    drop(fleet);
    std::fs::create_dir(root.join("projects"))?;
    let broken = message_board::ProjectId::generate();
    std::fs::write(
        root.join("projects")
            .join(format!("{}.sqlite", broken.as_str())),
        b"corrupt native file",
    )?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: root.to_path_buf(),
        codex_home: root.to_path_buf(),
        backend_socket: root.join("absent-native.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: Some(root.join("test-peer-registry")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(root, "corrupt-project-isolation", "1").await?;
    let _endpoints = client.list_endpoints().await?;
    let _workspaces = client
        .workspace_list(WorkspaceListRequest {
            repository: None,
            page: PageRequest {
                limit: PageLimit::try_from(10)?,
                cursor: None,
            },
        })
        .await?;
    runtime.shutdown().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::indexing_slicing, clippy::panic_in_result_fn)]
async fn pending_corrupt_feed_is_contained_during_subscription_restart()
-> Result<(), board_fixture::TestError> {
    subscription_restart_contains_project_failure(RestartFault::PendingFeed).await
}

#[tokio::test]
async fn same_readers_corrupt_membership_is_contained_during_subscription_restart()
-> Result<(), board_fixture::TestError> {
    subscription_restart_contains_project_failure(RestartFault::Membership).await
}

#[derive(Clone, Copy)]
enum RestartFault {
    PendingFeed,
    Membership,
}

#[allow(clippy::indexing_slicing, clippy::panic_in_result_fn)]
async fn subscription_restart_contains_project_failure(
    fault: RestartFault,
) -> Result<(), board_fixture::TestError> {
    use board_fixture::{BoardFixture, accepted};
    use message_board::*;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let fixture = BoardFixture::start().await?;
    let (healthy, _) = fixture.project().await?;
    let (broken, _) = fixture.project().await?;
    let automation = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(
            &fixture._directory.path().join("automation.sqlite"),
        )
        .await?,
    ));
    let mut windows = fixture
        ._admission
        .subscription_window_store(automation.clone());
    for project in [&healthy, &broken] {
        let mut request = fixture.header();
        request["scope"] = json!({"kind":"project","projectId":project});
        request["policy"] = match fault {
            RestartFault::PendingFeed => json!({"mode":"poll"}),
            RestartFault::Membership => {
                json!({"mode":"poll","quietSeconds":0,"capSeconds":0})
            }
        };
        accepted(&fixture.api.call("thread_subscribe", request).await?);
    }
    // Durable progress precedes the pending posts, as when the process crashes before scanning.
    windows
        .restore_active_and_draining(chrono::Utc::now())
        .await?;
    let mut pending_roots = Vec::new();
    for project in [&healthy, &broken] {
        let mut request = fixture.header();
        request["projectId"] = json!(project);
        let post = PostId::generate();
        request["postId"] = json!(post);
        if matches!(fault, RestartFault::Membership) {
            request["actor"] = json!({"kind":"human","humanId":"pending-post-author"});
        }
        request["about"] = json!(fixture.discussion_subject(project).await?);
        request["text"] = json!("pending before crash");
        request["references"] = json!([]);
        request["role"] = json!(null);
        request["watch"] = json!(false);
        request["addressees"] = json!([]);
        accepted(&fixture.api.call("thread_create", request).await?);
        pending_roots.push((project.clone(), ThreadId::from(post)));
    }
    let root = fixture._directory.path().to_path_buf();
    fixture.api.stop().await?;
    drop(windows);
    drop(automation);
    for project in [&healthy, &broken] {
        fixture
            ._registry
            .existing_project(project)
            .await?
            .ok_or("project")?
            .close_connections()
            .await;
    }
    drop(fixture._admission);
    drop(fixture._registry);

    // Inject one fault only after closing the native stores. Reader/scope keys stay valid.
    use sqlx::Connection;
    let mut observer = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(
            root.join("projects")
                .join(format!("{}.sqlite", broken.as_str())),
        ),
    )
    .await?;
    let (position, entry): (i64, String) = sqlx::query_as(
        "SELECT position, entry FROM project_feed WHERE record_kind = 'post' ORDER BY position DESC LIMIT 1",
    ).fetch_one(&mut observer).await?;
    let mut entry: serde_json::Value = serde_json::from_str(&entry)?;
    assert_eq!(
        entry.pointer("/content/record/record/text"),
        Some(&json!("pending before crash"))
    );
    match fault {
        RestartFault::PendingFeed => {
            entry["content"]["record"]["record"]["text"] = json!("");
            sqlx::query("UPDATE project_feed SET entry = ? WHERE position = ?")
                .bind(serde_json::to_string(&entry)?)
                .bind(position)
                .execute(&mut observer)
                .await?;
        }
        RestartFault::Membership => {
            let payload: String = sqlx::query_scalar(
                "SELECT payload FROM record_identities WHERE kind='subscriptionMembership'",
            )
            .fetch_one(&mut observer)
            .await?;
            let mut payload: serde_json::Value = serde_json::from_str(&payload)?;
            assert_eq!(payload.pointer("/record/reader"), Some(&fixture.actor));
            payload["record"]["policy"]["mode"] = json!("corrupt-mode");
            let changed = sqlx::query(
                "UPDATE record_identities SET payload=? WHERE kind='subscriptionMembership'",
            )
            .bind(serde_json::to_string(&payload)?)
            .execute(&mut observer)
            .await?;
            assert_eq!(changed.rows_affected(), 1);
        }
    }
    observer.close().await?;

    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("absent-native.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: Some(root.join("test-peer-registry")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(&root, "pending-feed-isolation", "1").await?;
    client.list_endpoints().await?;
    let reply = client
        .workstream_create(ProjectRequest {
            request_id: RequestId::generate(),
            project_id: healthy.clone(),
            actor: serde_json::from_value(fixture.actor.clone())?,
            acting_for: None,
            observed_epoch: Some(EpochNumber::try_from(1_u64)?),
            resubmits: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: BoardOperation::WorkstreamCreate {
                workstream_id: WorkstreamId::generate(),
                name: "healthy after restart".to_owned().try_into()?,
                goal: "project failure stays contained".to_owned(),
            },
        })
        .await?;
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
    let shown = client
        .hook_card(ContinuationCardRequest {
            project_id: healthy.clone(),
            actor: serde_json::from_value(fixture.actor.clone())?,
            subject: TrailSubject::Project,
        })
        .await?;
    assert!(matches!(shown, HookResult::Ok(_)), "{shown:?}");
    let shown = client
        .hook_card(ContinuationCardRequest {
            project_id: broken.clone(),
            actor: serde_json::from_value(fixture.actor.clone())?,
            subject: TrailSubject::Project,
        })
        .await?;
    assert!(matches!(shown, HookResult::Unavailable { .. }), "{shown:?}");
    if matches!(fault, RestartFault::Membership) {
        let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
        let listed = client
            .thread_subscriptions(collaboration_protocol::ThreadSubscriptionsRequest {
                actor: reader.clone(),
            })
            .await?;
        assert_eq!(listed.subscriptions.len(), 1);
        assert_eq!(listed.subscriptions[0].project_id, healthy);
        assert_eq!(listed.subscriptions[0].pending_count, 1);
        let delivered = client
            .thread_wait(
                collaboration_protocol::ThreadSubscriptionWaitRequest {
                    actor: reader.clone(),
                    acting_for: None,
                    protocol_version: ProtocolVersion::CURRENT,
                    renewals: Vec::new(),
                    filter: collaboration_protocol::ThreadSubscriptionWaitFilter::Project {
                        project_id: healthy.clone(),
                    },
                    max_wait_seconds: 1,
                },
                std::time::Duration::from_secs(5),
            )
            .await?;
        let collaboration_protocol::ThreadSubscriptionWaitOutcome::Batch {
            batch: collaboration_protocol::SubscriptionWaitBatch::Ranges { roots, .. },
        } = delivered.outcome
        else {
            return Err("healthy reader's restored poll delivery was lost".into());
        };
        let healthy_root = pending_roots
            .iter()
            .find(|(project, _)| project == &healthy)
            .map(|(_, thread)| thread)
            .ok_or("healthy root")?;
        assert_eq!(roots.len(), 1);
        assert_eq!(&roots[0].thread_id, healthy_root);
        let direct = message_board_storage::ProjectStore::open(
            &root
                .join("projects")
                .join(format!("{}.sqlite", broken.as_str())),
            broken.clone(),
        )
        .await?;
        let mut read = direct.snapshot().await?;
        let corruption = match read
            .membership(&reader, &SubscriptionScope::project(broken))
            .await
        {
            Err(error) => error,
            Ok(_) => return Err("direct membership read accepted the corrupt body".into()),
        };
        assert!(matches!(
            corruption,
            message_board_storage::ProjectStorageError::InvalidRecord {
                field: "subscriptionMembership.policy.mode",
                ..
            }
        ));
        read.finish().await?;
        direct.close().await;
    }
    runtime.shutdown().await?;
    Ok(())
}
