//! The Host reads no peer registry unless one is configured.
use super::*;

type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error>>;

#[test]
fn host_peer_registry_default_is_none_without_reading_a_registry() -> TestResult {
    let config = host_config(
        Path::new("/tmp/inert-registry-default"),
        "127.0.0.1:9".parse()?,
    );
    check(
        config.collaboration_peer_registry_directory().is_none(),
        "default registry must be None",
    )
}

fn host_config(root: &Path, router: std::net::SocketAddr) -> HostConfig {
    HostConfig::new(HostConfigInputs {
        coordination_paths: HostCoordinationPaths::new(
            root.join("operator.sock"),
            root.join("host.lock"),
        ),
        router_endpoint: router,
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        app_server_socket: root.join("native.sock"),
        managed_executable: root.join("managed-codex"),
        deadlines: HostDeadlines::production(),
    })
    .with_collaboration_directory(root.join("api"), root.join("codex-home"))
}
