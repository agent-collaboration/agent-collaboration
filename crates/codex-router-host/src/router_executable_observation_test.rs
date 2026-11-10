use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[tokio::test]
async fn same_file_is_a_match() {
    let path = std::env::current_exe().expect("test executable");
    let mut observer = RouterExecutableObserver::capture(Ok(path), "test-running".to_owned());
    assert_eq!(observer.observe().await, RouterExecutableRelation::Match);
    assert_eq!(observer.relation(), RouterExecutableRelation::Match);
}

#[tokio::test]
async fn replaced_file_with_a_different_version_is_drift() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let launch_path = directory.path().join("codex-router");
    write_version_script(&launch_path, "0.1.36");
    let startup_metadata = std::fs::metadata(&launch_path).expect("initial script metadata");
    let startup_modified_time = startup_metadata.modified().expect("initial script mtime");
    let mut observer =
        RouterExecutableObserver::capture(Ok(launch_path.clone()), "0.1.36".to_owned());
    assert_eq!(observer.observe().await, RouterExecutableRelation::Match);
    replace_version_script(&launch_path, "0.1.37");
    restore_modified_time(&launch_path, startup_modified_time);
    assert_eq!(
        std::fs::metadata(&launch_path)
            .expect("updated script metadata")
            .len(),
        startup_metadata.len()
    );
    observer.installed_version_override = Some("0.1.37".to_owned());
    assert_eq!(
        observer.observe().await,
        RouterExecutableRelation::Drift {
            running_version: "0.1.36".to_owned(),
            installed_version: Some("0.1.37".to_owned())
        }
    );
}

#[tokio::test]
async fn replacement_with_the_same_version_remains_a_match() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let launch_path = directory.path().join("codex-router");
    write_version_script(&launch_path, "0.1.36");
    let startup_modified_time = std::fs::metadata(&launch_path)
        .expect("initial script metadata")
        .modified()
        .expect("initial script mtime");
    let mut observer =
        RouterExecutableObserver::capture(Ok(launch_path.clone()), "0.1.36".to_owned());
    assert_eq!(observer.observe().await, RouterExecutableRelation::Match);
    replace_version_script(&launch_path, "0.1.36");
    restore_modified_time(&launch_path, startup_modified_time);
    observer.installed_version_override = Some("0.1.36".to_owned());
    assert_eq!(observer.observe().await, RouterExecutableRelation::Match);
}

#[tokio::test]
async fn missing_launch_path_is_drift() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("router");
    std::fs::write(&path, b"router").expect("write router");
    let mut observer = RouterExecutableObserver::capture(Ok(path.clone()), "0.1.36".to_owned());
    std::fs::remove_file(path).expect("remove launch file");
    assert_eq!(
        observer.observe().await,
        RouterExecutableRelation::Drift {
            running_version: "0.1.36".to_owned(),
            installed_version: None
        }
    );
}

#[tokio::test]
async fn unreadable_launch_path_is_unknown() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("router");
    std::fs::create_dir(&path).expect("create unreadable executable directory");
    // Keep size and mtime stable so only the replacement inode distinguishes it.
    std::fs::write(path.join("initial"), b"changed").expect("initial directory entry");
    let startup_metadata = std::fs::metadata(&path).expect("initial directory metadata");
    let startup_modified_time = startup_metadata
        .modified()
        .expect("initial directory mtime");
    let mut observer = RouterExecutableObserver::capture(Ok(path.clone()), "0.1.36".to_owned());
    let replacement_path = directory.path().join("replacement-router");
    std::fs::create_dir(&replacement_path).expect("create replacement directory");
    std::fs::write(replacement_path.join("changed"), b"changed")
        .expect("replacement directory entry");
    std::fs::rename(&path, directory.path().join("previous-router"))
        .expect("retain original directory inode");
    std::fs::rename(&replacement_path, &path).expect("replace launch directory inode");
    restore_modified_time(&path, startup_modified_time);
    assert_ne!(
        std::fs::metadata(&path)
            .expect("replacement directory metadata")
            .ino(),
        startup_metadata.ino()
    );
    assert_eq!(
        std::fs::metadata(&path)
            .expect("updated directory metadata")
            .len(),
        startup_metadata.len()
    );
    assert!(matches!(
        observer.observe().await,
        RouterExecutableRelation::Unknown { .. }
    ));
}

fn restore_modified_time(path: &std::path::Path, modified: std::time::SystemTime) {
    std::fs::File::open(path)
        .expect("open fixture metadata handle")
        .set_modified(modified)
        .expect("restore fixture mtime");
    assert_eq!(
        std::fs::metadata(path)
            .expect("fixture metadata")
            .modified()
            .expect("fixture mtime"),
        modified
    );
}

fn replace_version_script(path: &std::path::Path, version: &str) {
    let original_inode = std::fs::metadata(path)
        .expect("initial script metadata")
        .ino();
    let replacement_path = path.with_extension("replacement");
    write_version_script(&replacement_path, version);
    std::fs::rename(replacement_path, path).expect("replace launch script inode");
    assert_ne!(
        std::fs::metadata(path)
            .expect("replacement script metadata")
            .ino(),
        original_inode
    );
}

fn write_version_script(path: &std::path::Path, version: &str) {
    std::fs::write(
        path,
        format!("#!/bin/sh\nprintf 'codex-router {version}\\n'\n"),
    )
    .expect("write version script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("make script executable");
}

/// A fresh client initializes the real collaboration API, which asks the Host's observer.
async fn mcp_initialize_instructions(address: std::net::SocketAddr) -> Option<String> {
    let response: serde_json::Value = reqwest::Client::new()
        .post(format!(
            "http://{address}{}",
            collaboration_mcp::COLLABORATION_API_PATH
        ))
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .header("mcp-protocol-version", "2025-11-25")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "stale-banner-proof", "version": "1"}
            }
        }))
        .send()
        .await
        .expect("POST initialize")
        .json()
        .await
        .expect("initialize JSON");
    response["result"]["instructions"]
        .as_str()
        .map(str::to_owned)
}

#[tokio::test]
async fn a_fresh_mcp_initialize_reports_an_atomically_replaced_router_as_stale() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let launch_path = directory.path().join("codex-router");
    write_version_script(&launch_path, "0.1.36");
    let observer = std::sync::Arc::new(tokio::sync::Mutex::new(RouterExecutableObserver::capture(
        Ok(launch_path.clone()),
        "0.1.36".to_owned(),
    )));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bound listener");
    let address = listener.local_addr().expect("listener address");
    let shutdown = tokio_util::sync::CancellationToken::new();
    let config = collaboration_mcp::CollaborationApiConfig {
        application: collaboration_service::ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )
        .expect("identity")
        .build(),
        service_directory: directory.path().to_owned(),
        native_definitions: None,
        router_executable_check: std::sync::Arc::new(SharedRouterExecutableCheck(
            std::sync::Arc::clone(&observer),
        )),
        concurrent_requests: collaboration_mcp::DEFAULT_CONCURRENT_REQUESTS,
        shutdown: shutdown.clone(),
    };
    let router = collaboration_mcp::collaboration_api_router(
        &config,
        collaboration_mcp::CollaborationApiListener::LoopbackTcp {
            address,
            proxy_authority: None,
        },
    );
    let server = tokio::spawn(collaboration_mcp::serve_collaboration_api(
        listener,
        router,
        shutdown.clone(),
    ));
    let before = mcp_initialize_instructions(address).await;

    replace_version_script(&launch_path, "0.1.37");
    let after = mcp_initialize_instructions(address).await;

    assert_eq!(before, None);
    assert_eq!(
        after.as_deref(),
        Some(
            "⚠ Router Host is stale (running 0.1.36, installed 0.1.37); run `codex-router host restart`"
        )
    );
    shutdown.cancel();
    server.await.expect("server task").expect("server shutdown");
}
