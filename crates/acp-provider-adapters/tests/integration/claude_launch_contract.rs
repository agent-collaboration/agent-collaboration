use acp_client_runtime::{ExternalProviderLaunch, ProviderPersistenceTarget};
use acp_provider_adapters::{ClaudeSkillSource, prepare_claude_launch};
use std::path::{Path, PathBuf};

struct Installation {
    directory: tempfile::TempDir,
    entry: PathBuf,
    library: PathBuf,
    shim: PathBuf,
}

fn installation() -> Result<Installation, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let peer = root.join("store/acp/node_modules");
    let package = peer.join("@agentclientprotocol/claude-agent-acp");
    let sdk = root.join("store/sdk/node_modules/@anthropic-ai/claude-agent-sdk");
    std::fs::create_dir_all(package.join("dist"))?;
    std::fs::create_dir_all(&sdk)?;
    std::fs::create_dir_all(peer.join("@anthropic-ai"))?;
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"@agentclientprotocol/claude-agent-acp","version":"0.81.0","type":"module","bin":{"claude-agent-acp":"dist/index.js"},"exports":{".":{"import":"./dist/lib.js"}}}"#,
    )?;
    std::fs::write(
        sdk.join("package.json"),
        r#"{"name":"@anthropic-ai/claude-agent-sdk","version":"0.3.280","type":"module","exports":{".":"./sdk.mjs"}}"#,
    )?;
    std::fs::write(sdk.join("sdk.mjs"), "export function resolveSettings() {}")?;
    std::fs::write(package.join("dist/lib.js"), "export function runAcp() {}")?;
    std::fs::write(package.join("dist/index.js"), "#!/usr/bin/env node\n")?;
    std::os::unix::fs::symlink(&sdk, peer.join("@anthropic-ai/claude-agent-sdk"))?;
    let logical = root.join("global/node_modules/@agentclientprotocol");
    std::fs::create_dir_all(&logical)?;
    std::os::unix::fs::symlink(&package, logical.join("claude-agent-acp"))?;
    let entry = logical.join("claude-agent-acp/dist/index.js");
    let shim = root.join("claude-agent-acp");
    std::fs::write(
        &shim,
        format!("#!/bin/sh\n# cmd-shim-target={}\n", entry.display()),
    )?;
    Ok(Installation {
        directory,
        entry,
        library: package.join("dist/lib.js"),
        shim,
    })
}

fn launch(executable: &Path) -> ExternalProviderLaunch {
    ExternalProviderLaunch {
        executable: executable.into(),
        arguments: vec![],
        environment: vec![("ROUTER_NONSECRET_FIXTURE".into(), "preserve".into())],
        persistence_target: ProviderPersistenceTarget::ClaudeSettingsRule,
    }
}

#[tokio::test]
async fn official_direct_and_pnpm_shim_use_canonical_peer_sdk_and_preserve_environment() {
    let fixture = installation().expect("pnpm installation fixture");
    for executable in [&fixture.entry, &fixture.shim] {
        let original = launch(executable);
        let prepared = prepare_claude_launch(original.clone()).await;
        let identity = prepared.qualification.expect("qualified public imports");
        assert_eq!(
            identity.library,
            std::fs::canonicalize(&fixture.library).expect("canonical fixture library")
        );
        assert!(matches!(
            prepared.skill_source,
            ClaudeSkillSource::Annotated
        ));
        assert_eq!(prepared.launch.environment, original.environment);
        assert_eq!(
            prepared.launch.persistence_target,
            original.persistence_target
        );
        assert_eq!(prepared.launch.executable, identity.node);
        assert_eq!(
            prepared.launch.arguments.first().map(String::as_str),
            Some("--input-type=module")
        );
        assert!(
            prepared
                .launch
                .arguments
                .iter()
                .any(|argument| argument.contains("sessionUpdate"))
        );
        assert!(prepared.launch.arguments.last().is_some_and(|argument| {
            argument.starts_with("file:") && argument.ends_with("/sdk.mjs")
        }));
    }
}

#[tokio::test]
async fn custom_launch_version_and_unsupported_arguments_remain_original_and_unavailable() {
    let fixture = installation().expect("installation");
    let custom = fixture.directory.path().join("custom");
    std::fs::write(&custom, "#!/bin/sh\nexit 0\n").expect("custom executable");
    let mut with_args = launch(&fixture.entry);
    with_args.arguments = vec!["--cli".into()];
    for original in [launch(&custom), with_args] {
        let prepared = prepare_claude_launch(original.clone()).await;
        assert!(prepared.qualification.is_err());
        assert_eq!(prepared.launch, original);
        assert!(matches!(
            prepared.skill_source,
            ClaudeSkillSource::Unavailable(_)
        ));
    }
    std::fs::write(
        fixture
            .library
            .parent()
            .expect("dist")
            .parent()
            .expect("package")
            .join("package.json"),
        r#"{"name":"@agentclientprotocol/claude-agent-acp","version":"0.82.0"}"#,
    )
    .expect("changed version");
    let original = launch(&fixture.entry);
    let prepared = prepare_claude_launch(original.clone()).await;
    assert!(prepared.qualification.is_err());
    assert_eq!(prepared.launch, original);
}

#[tokio::test]
async fn stalled_qualification_does_not_block_another_provider_and_keeps_original_launch() {
    let stalled = installation().expect("stalled fixture");
    std::fs::write(
        &stalled.library,
        "await new Promise(() => { setInterval(() => {}, 1000); }); export function runAcp() {}",
    )
    .expect("held import");
    let healthy = installation().expect("healthy fixture");
    let original = launch(&stalled.entry);
    let pending = prepare_claude_launch(original.clone());
    tokio::pin!(pending);
    tokio::select! {
        biased;
        _result = &mut pending => panic!("stalled import completed before independent qualification"),
        prepared = prepare_claude_launch(launch(&healthy.entry)) => assert!(prepared.qualification.is_ok()),
    }
    let prepared = pending.await;
    assert!(prepared.qualification.is_err());
    assert_eq!(prepared.launch, original);
    assert!(matches!(
        prepared.skill_source,
        ClaudeSkillSource::Unavailable(_)
    ));
}
