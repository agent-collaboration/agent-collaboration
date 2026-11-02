//! The interim default is checked against the service loaded by the actual Host.
use super::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_protocol::{RouterExecutableRelation, SessionRef};
use std::{os::unix::fs::PermissionsExt as _, path::Path};

fn chief(service: &collaboration_protocol::UuidIdentity) -> SessionRef {
    serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"chief"
    }))
    .expect("Chief identity")
}

async fn start(root: &Path, approver: SessionRef) -> std::io::Result<CollaborationRuntime> {
    let (_sender, relation) = tokio::sync::watch::channel(RouterExecutableRelation::Match);
    Box::pin(
        CollaborationRuntime::start_with_optional_router_proxy_endpoint(
            CollaborationRuntimeInputs {
                board_session: None,
                owner_human_id: None,
                directory: root.to_owned(),
                codex_home: root.to_owned(),
                backend_socket: root.join("missing-native.sock"),
                mcp_bind: ([127, 0, 0, 1], 0).into(),
                native_schema: None,
                peer_registry_directory: Some(root.join("peers")),
                remote_control_server_name: None,
            },
            Vec::new(),
            relation,
            None,
            None,
            Some(approver),
        ),
    )
    .await
}

fn private_root() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("chief-config-")
        .tempdir_in("/tmp")
        .expect("private fixture root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private root");
    std::fs::create_dir(root.path().join("peers")).expect("private peer fixture");
    root
}

#[tokio::test]
async fn host_retains_its_local_interim_chief_without_a_new_configuration_store() {
    let root = private_root();
    let service =
        collaboration_runtime::load_service_identity(root.path()).expect("persisted local service");
    let expected = chief(&service);
    let config = crate::HostConfig::new(crate::HostConfigInputs {
        coordination_paths: crate::HostCoordinationPaths::for_router_root(root.path()),
        router_endpoint: ([127, 0, 0, 1], 0).into(),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        app_server_socket: root.path().join("native.sock"),
        managed_executable: "fixture-codex".into(),
        deadlines: crate::HostDeadlines::production(),
    });
    assert!(config.remote_create_default_approver().is_none());
    let config = config.with_remote_create_default_approver(expected.clone());
    assert_eq!(config.remote_create_default_approver(), Some(&expected));
    let runtime = start(
        root.path(),
        config
            .remote_create_default_approver()
            .expect("configured Chief")
            .clone(),
    )
    .await
    .expect("local Chief starts");
    assert_eq!(runtime.remote_create_default_approver(), Some(&expected));
    runtime.shutdown().await.expect("fixture Host shuts down");
}

#[tokio::test]
async fn foreign_interim_chief_is_refused_before_the_host_publishes_its_api() {
    let root = private_root();
    let other = private_root();
    let foreign =
        collaboration_runtime::load_service_identity(other.path()).expect("other service");
    let refused = match start(root.path(), chief(&foreign)).await {
        Err(error) => error.kind() == std::io::ErrorKind::InvalidInput,
        Ok(runtime) => {
            runtime
                .shutdown()
                .await
                .expect("retire unexpected admission");
            false
        }
    };
    assert!(refused, "a foreign Chief must fail at Host load");
    assert!(!root.path().join("service.json").exists());
}
