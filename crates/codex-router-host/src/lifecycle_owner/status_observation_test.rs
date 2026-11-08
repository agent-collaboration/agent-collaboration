use super::observe_status;
use crate::{HostConfig, RouterExecutableObserver, RouterExecutableRelation};
use std::os::unix::fs::PermissionsExt as _;
use std::sync::Arc;

fn write_version_script(path: &std::path::Path, version: &str) {
    std::fs::write(
        path,
        format!("#!/bin/sh\nprintf 'codex-router {version}\\n'\n"),
    )
    .expect("write version script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("make script executable");
}

/// Installs a new executable the way an upgrade does: a new file renamed over the old one.
fn replace_atomically(path: &std::path::Path, version: &str) {
    let replacement = path.with_extension("replacement");
    write_version_script(&replacement, version);
    std::fs::rename(replacement, path).expect("rename the new executable into place");
}

fn status_config(root: &std::path::Path) -> HostConfig {
    // Nothing listens on these loopback ports, so the Router probe answers at once.
    let unused = std::net::TcpListener::bind("127.0.0.1:0").expect("pick a port");
    let router_endpoint = unused.local_addr().expect("port");
    drop(unused);
    HostConfig::new(crate::HostConfigInputs {
        coordination_paths: crate::HostCoordinationPaths::for_router_root(root),
        router_endpoint,
        mcp_bind: "127.0.0.1:0".parse().expect("loopback"),
        app_server_socket: root.join("app.sock"),
        managed_executable: std::path::PathBuf::from("/bin/sh"),
        deadlines: crate::HostDeadlines::production(),
    })
}

#[tokio::test]
async fn host_status_reports_a_router_replaced_since_start_with_no_timer() {
    let root = tempfile::tempdir().expect("status root");
    let launch_path = root.path().join("codex-router");
    write_version_script(&launch_path, "0.1.36");
    let observer = Arc::new(tokio::sync::Mutex::new(RouterExecutableObserver::capture(
        Ok(launch_path.clone()),
        "0.1.36".to_owned(),
    )));
    let status = || {
        observe_status(
            status_config(root.path()),
            crate::RouterOwnership::External,
            false,
            None,
            std::time::Duration::from_secs(2),
            false,
            Arc::clone(&observer),
        )
    };
    let before = status().await.router_executable_relation();

    replace_atomically(&launch_path, "0.1.37");
    let after = status().await.router_executable_relation();

    assert_eq!(before, RouterExecutableRelation::Match);
    assert_eq!(
        after,
        RouterExecutableRelation::Drift {
            running_version: "0.1.36".to_owned(),
            installed_version: Some("0.1.37".to_owned()),
        }
    );
}
