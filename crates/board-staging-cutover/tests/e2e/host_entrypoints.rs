//! Only owned child selectors run these fixture entry points; no default Host is used.
use codex_router_host::{
    AppServerLaunchPlan, ChildCommandSpec, ChildOutput, HostConfig, HostConfigInputs,
    HostCoordinationPaths, HostDeadlines, HostRuntime, ManagedChildLaunchPlans,
    ManagedUpdateInputs,
};
use std::path::{Path, PathBuf};

pub(super) const ROOT_ENV: &str = "C5B_OWNED_HOST_ROOT";
pub(super) const ROUTER_ENV: &str = "C5B_OWNED_ROUTER_ADDRESS";
pub(super) const OWNER_ENV: &str = "C5B_OWNED_OWNER_ID";
pub(super) const HOST_BASE_PREFIX: &str = "2026-11-c5b-";

fn declared_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).ok_or("owned Host root missing")?);
    let actual = std::fs::canonicalize(&root)?;
    let base = actual
        .parent()
        .and_then(Path::parent)
        .ok_or("owned Host base missing")?;
    if actual != root
        || base.parent() != Some(std::fs::canonicalize("/tmp")?.as_path())
        || !base
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(HOST_BASE_PREFIX))
    {
        return Err("refusing a Host outside the owned cutover fixture directory".into());
    }
    Ok(root)
}

#[tokio::test]
#[ignore = "owned Host child selected and asserted by the cutover process proof"]
async fn owned_host_child() -> Result<(), Box<dyn std::error::Error>> {
    let root = declared_root()?;
    let executable = std::env::current_exe()?;
    let identity = codex_native_integration::executable_identity(&executable).await?;
    let native = root.join("native.sock");
    let command = ChildCommandSpec::new(executable.clone())
        .with_arguments([
            "--exact",
            "host_entrypoints::owned_native_child",
            "--ignored",
            "--nocapture",
        ])
        .with_environment(ROOT_ENV, &root)
        .with_output(ChildOutput::Inherit);
    let config = HostConfig::new(HostConfigInputs {
        coordination_paths: HostCoordinationPaths::for_router_root(&root),
        router_endpoint: std::env::var(ROUTER_ENV)?.parse()?,
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        app_server_socket: native,
        managed_executable: executable,
        deadlines: HostDeadlines::production(),
    })
    .with_collaboration_directory(
        codex_router_host::router_collaboration_directory(&root),
        root.join("fixture-native-home"),
    )
    .with_owner_human_id(std::env::var(OWNER_ENV)?.try_into()?);
    if !config.external_provider_startups().is_empty() {
        return Err("owned cutover Host must not launch provider processes".into());
    }
    Box::pin(HostRuntime::run(
        config,
        ManagedChildLaunchPlans::new(
            None,
            AppServerLaunchPlan::new(command, identity, "1.2.3".to_owned()),
        ),
        ManagedUpdateInputs::production(),
    ))
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "owned native protocol child selected by the cutover Host fixture"]
async fn owned_native_child() -> Result<(), Box<dyn std::error::Error>> {
    let root = declared_root()?;
    codex_router_test_support::native_app_server::run_native_app_server_fixture(
        &root.join("native.sock"),
        "1.2.3",
        Some(&root.join("native-pids.log")),
    )
    .await
}
