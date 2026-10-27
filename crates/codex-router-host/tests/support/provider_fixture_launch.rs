//! Launches repository-owned TypeScript ACP fixtures for Host integration tests.

pub fn launch(program: &str, arguments: Vec<String>) -> codex_router_host::ExternalProviderLaunch {
    let script = codex_router_test_support::node_fixture::fixture_program(program);
    codex_router_host::ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: std::path::PathBuf::from("node"),
        arguments: std::iter::once(script.to_string_lossy().into_owned())
            .chain(arguments)
            .collect(),
        environment: Vec::new(),
    }
}
