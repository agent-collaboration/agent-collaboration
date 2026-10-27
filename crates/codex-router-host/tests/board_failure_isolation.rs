#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::large_futures,
    reason = "lint pass follows the crate split"
)]
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{BoardFailureKind, PageLimit, PageRequest, WorkspaceListRequest};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
// This Host restart proof uses the shared board setup, not every fixture journey.
#[expect(
    unused_imports,
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod board_fixture;
#[path = "support/native_project_fixture.rs"]
mod native_project_fixture;

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
    std::fs::create_dir(root.join("fleet-cache.sqlite"))?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
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
    let fleet = collaboration_runtime::FleetClient::open(
        root,
        std::sync::Arc::new(native_project_fixture::UnsettledRegistration),
        None,
    )
    .await?;
    fleet.close().await;
    std::fs::create_dir(root.join("projects"))?;
    let broken = message_board::ProjectId::generate();
    let project_directory = root.join("projects").join(broken.as_str());
    std::fs::create_dir(&project_directory)?;
    std::fs::write(project_directory.join("e1.sqlite"), b"corrupt native file")?;
    let offline_endpoint = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: Some(native_project_fixture::offline_inputs(
            offline_endpoint.local_addr()?,
            collaboration_runtime::load_installation_peer(root)?,
        )?),
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

async fn pending_corrupt_feed_is_contained_during_subscription_restart()
-> Result<(), board_fixture::TestError> {
    Box::pin(subscription_restart_contains_project_failure(
        RestartFault::PendingFeed,
    ))
    .await
}

#[tokio::test]
async fn same_readers_corrupt_membership_is_contained_during_subscription_restart()
-> Result<(), board_fixture::TestError> {
    Box::pin(subscription_restart_contains_project_failure(
        RestartFault::Membership,
    ))
    .await
}

#[derive(Clone, Copy)]
enum RestartFault {
    PendingFeed,
    Membership,
}

#[expect(
    clippy::indexing_slicing,
    reason = "The test keeps its existing explicit assertion and failure contract."
)]
async fn subscription_restart_contains_project_failure(
    fault: RestartFault,
) -> Result<(), board_fixture::TestError> {
    use board_fixture::{BoardFixture, accepted};
    use message_board::*;
    use serde_json::json;
    use std::sync::Arc;

    let fixture = BoardFixture::start().await?;
    // The native fixture's grants use this peer; the restarted Host must load it too.
    let fixture_peer = board_fixture::fixture_peer()?;
    let identity_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(fixture._directory.path().join("installation-peer.json"))?;
    serde_json::to_writer(&identity_file, &json!({"version":1,"peerId":fixture_peer}))?;
    identity_file.sync_all()?;
    drop(identity_file);
    assert_eq!(
        collaboration_runtime::load_installation_peer(fixture._directory.path())?,
        fixture_peer
    );
    let (healthy, _) = fixture.project().await?;
    let (broken, _) = fixture.project().await?;
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(
            fixture._directory.path().join("interaction.sqlite"),
        )
        .await?,
    );
    let mut windows = fixture
        ._admission
        .subscription_window_store(interactions.clone());
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
    Arc::try_unwrap(interactions)
        .map_err(|_| "fixture retained worker owner")?
        .close()
        .await?;
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
                .join(broken.as_str())
                .join("e1.sqlite"),
        ),
    )
    .await?;
    let (position, content): (i64, String) = sqlx::query_as(
        "SELECT position, content FROM project_feed WHERE record_kind = 'post' ORDER BY position DESC LIMIT 1",
    ).fetch_one(&mut observer).await?;
    let mut content: serde_json::Value = serde_json::from_str(&content)?;
    assert_eq!(
        content.pointer("/record/record/text"),
        Some(&json!("pending before crash"))
    );
    match fault {
        RestartFault::PendingFeed => {
            content["record"]["record"]["text"] = json!("");
            let changed = sqlx::query("UPDATE project_feed SET content = ? WHERE position = ?")
                .bind(serde_json::to_string(&content)?)
                .bind(position)
                .execute(&mut observer)
                .await?;
            assert_eq!(changed.rows_affected(), 1);
        }
        RestartFault::Membership => {
            let (reader, policy): (String, String) = sqlx::query_as(
                "SELECT a.canonical_identity,m.policy FROM subscription_memberships m JOIN actors a ON a.actor_id=m.reader_actor",
            )
            .fetch_one(&mut observer)
            .await?;
            let reader: RecordedActor = serde_json::from_str(&reader)?;
            assert_eq!(
                reader.declared_identity(),
                Some(serde_json::from_value(fixture.actor.clone())?)
            );
            let mut policy: serde_json::Value = serde_json::from_str(&policy)?;
            policy["mode"] = json!("corrupt-mode");
            let changed = sqlx::query("UPDATE subscription_memberships SET policy=?")
                .bind(serde_json::to_string(&policy)?)
                .execute(&mut observer)
                .await?;
            assert_eq!(changed.rows_affected(), 1);
        }
    }
    observer.close().await?;

    let offline_endpoint = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: Some(native_project_fixture::offline_inputs(
            offline_endpoint.local_addr()?,
            collaboration_runtime::load_installation_peer(&root)?,
        )?),
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
            collaboration_client::board::RequestResult::Decided(
                collaboration_client::board::RequestDecision::Accepted { .. }
            )
        ),
        "{reply:?}"
    );
    let shown = client
        .view_card(ContinuationCardRequest {
            project_id: healthy.clone(),
            actor: serde_json::from_value(fixture.actor.clone())?,
            subject: TrailSubject::Project,
        })
        .await?;
    assert_eq!(shown.subject, healthy.clone().into());
    let shown = client
        .view_card(ContinuationCardRequest {
            project_id: broken.clone(),
            actor: serde_json::from_value(fixture.actor.clone())?,
            subject: TrailSubject::Project,
        })
        .await;
    assert!(shown.is_err(), "{shown:?}");
    let mut direct_reader = None;
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
        direct_reader = Some(reader);
    }
    runtime.shutdown().await?;
    // The native file has one writer owner. Inspect its selected corruption
    // through an owning reopen after Host shutdown, preserving the public
    // isolation and healthy-delivery assertions made above while it was live.
    if let Some(reader) = direct_reader {
        let registry = native_project_fixture::registry(&root)?;
        let direct = registry
            .existing_project(&broken)
            .await?
            .ok_or("broken project file")?;
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
        drop(direct);
        Arc::try_unwrap(registry)
            .map_err(|_| std::io::Error::other("direct native registry still owned"))?
            .close()
            .await?;
    }
    Ok(())
}
