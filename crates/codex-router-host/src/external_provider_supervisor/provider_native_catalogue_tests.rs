//! Wiring regressions: real supervisor/listener/readers, controlled CLI export.
//! The export process is a contract stand-in, never installed-native proof.
#![allow(clippy::expect_used)] // Fixture construction follows the existing Host integration-test convention.
use super::*;
use crate::{
    ExternalProviderBinding, ExternalProviderLaunch, HostSessionCommandPort,
    ProviderAcpDeliveryRoute,
};
use collaboration_protocol::ProviderKind;
use collaboration_service::{
    EndpointDirectory, ProviderSessionEventHub, RouterSessionAppServerContext,
    RouterSessionAppServerListener,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_tungstenite::{client_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

#[path = "provider_native_selection_tests.rs"]
mod selected_tests;
#[path = "provider_native_catalogue_table_tests.rs"]
mod table_tests;

const ACP_FIXTURE: &str = r#"
IFS= read -r request || exit 1
request_id=$(printf '%s\n' "$request" | sed -n 's/.*"id":\([^,}]*\).*/\1/p')
printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true}},"agentInfo":{"name":"catalogue-transport-fixture","version":"1"}}}\n' "$request_id"
prompt_count=0
while IFS= read -r request; do
  printf '%s\n' "$request" >> "$ACP_TRACE_FILE"
  request_id=$(printf '%s\n' "$request" | sed -n 's/.*"id":\([^,}]*\).*/\1/p')
  case "$request" in
    *'"method":"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"selection-fixture-session"}}\n' "$request_id" ;;
    *'"method":"session/prompt"'*)
      prompt_count=$((prompt_count + 1))
      prompt_text=$(printf '%s\n' "$request" | sed -En 's/.*"text":("(\\.|[^"\\])*").*/\1/p')
      test -n "$prompt_text" || exit 24
      if test -f "$ACP_NO_USER_ECHO_FILE"; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"selection-fixture-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Fixture native reply without user echo"}}}}\n'
      else
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"selection-fixture-session","update":{"sessionUpdate":"user_message_chunk","messageId":"native-catalogue-user-%s","content":{"type":"text","text":%s}}}}\n' "$prompt_count" "$prompt_text"
      fi
      if test -f "$ACP_HOLD_PROMPT_FILE"; then
        pending_prompt=$request_id
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"selection-fixture-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Fixture held prompt received"}}}}\n'
      else printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$request_id"; fi ;;
    *'"method":"session/cancel"'*)
      if test -n "$pending_prompt"; then printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$pending_prompt"; pending_prompt=; fi ;;
  esac
done
"#;
const NATIVE_EXPORT_FIXTURE: &str = r#"#!/bin/sh
printf '%s\n' "$1" >> "$NATIVE_EXPORT_TRACE"
case "$1" in
  --version) printf '2.1.310 (Claude Code)\n' ;;
  --setting-sources) cat "$NATIVE_EXPORT_FILE" ;;
  *) exit 23 ;;
esac
"#;

struct NoLivePeer;
impl crate::LiveSessionOwnershipCheck for NoLivePeer {
    fn check<'a>(
        &'a self,
        _target: &'a collaboration_protocol::SessionRef,
    ) -> collaboration_service::DeliveryFuture<'a, crate::LiveSessionOwnership> {
        Box::pin(async { Ok(crate::LiveSessionOwnership::NotLive) })
    }
}

struct CatalogueFixture {
    _directory: tempfile::TempDir,
    config: PathBuf,
    workspace: PathBuf,
    export: PathBuf,
    trace: PathBuf,
    native_export_trace: PathBuf,
    hold_prompt: PathBuf,
    socket: PathBuf,
    endpoint: collaboration_protocol::EndpointRef,
    board_endpoint: message_board::SessionEndpointRef,
    supervisor: Arc<ExternalProviderSupervisor>,
    commands: Arc<HostSessionCommandPort>,
    hub: Arc<ProviderSessionEventHub>,
    shutdown: CancellationToken,
    server: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl CatalogueFixture {
    async fn new(provider: ProviderKind) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir_in("/tmp").expect("owned private fixture");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .expect("private socket parent");
        let home = directory.path().join("home");
        let config = directory.path().join("config");
        let workspace = directory.path().join("workspace");
        for path in [&home, &config, &workspace, &workspace.join(".git")] {
            fs::create_dir_all(path).expect("native context directory");
        }
        let export = directory.path().join("native-export.json");
        fs::write(&export, "[]").expect("empty native package export");
        let trace = directory.path().join("acp-after-initialize.jsonl");
        fs::write(&trace, "").expect("owned ACP command trace");
        let native_export_trace = directory.path().join("native-export-calls.txt");
        fs::write(&native_export_trace, "").expect("owned export trace");
        let hold_prompt = directory.path().join("hold-prompt");
        let native_cli = directory.path().join("native-export-fixture");
        fs::write(&native_cli, NATIVE_EXPORT_FIXTURE).expect("export contract stand-in");
        fs::set_permissions(&native_cli, fs::Permissions::from_mode(0o700))
            .expect("owned executable");
        let mut environment = vec![
            ("HOME".into(), home.display().to_string()),
            ("CLAUDE_CONFIG_DIR".into(), config.display().to_string()),
            (
                "CLAUDE_CODE_EXECUTABLE".into(),
                native_cli.display().to_string(),
            ),
            ("NATIVE_EXPORT_FILE".into(), export.display().to_string()),
            ("ACP_TRACE_FILE".into(), trace.display().to_string()),
            (
                "ACP_NO_USER_ECHO_FILE".into(),
                directory.path().join("no-user-echo").display().to_string(),
            ),
            (
                "ACP_HOLD_PROMPT_FILE".into(),
                hold_prompt.display().to_string(),
            ),
            (
                "NATIVE_EXPORT_TRACE".into(),
                native_export_trace.display().to_string(),
            ),
        ];
        for name in [
            "CLAUDE_CODE_SIMPLE",
            "CLAUDE_CODE_SAFE_MODE",
            "CLAUDE_CODE_RESTRICTED",
            "CLAUDE_CODE_HOST_SKILL_CATALOG",
            "CLAUDE_CODE_USE_COWORK_PLUGINS",
            "CLAUDE_CODE_PROJECT_DIR_NAME",
            "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
        ] {
            environment.push((name.into(), String::new()));
        }
        let launch = ExternalProviderLaunch {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-c".into(), ACP_FIXTURE.into()],
            environment,
            persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        };
        let binding: ProviderBindingIdentity = serde_json::from_value(json!({
            "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":if provider == ProviderKind::ClaudeCode { "claude-local" } else { "cursor-local" }},
            "bindingId":"native-catalogue-fixture", "runtime":{"provider":provider,"runtimeName":"catalogue-fixture"},
            "transport":"stdioAcp", "generation":{"serviceEpoch":"1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80","generation":1},
            "capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]
        })).expect("binding");
        let endpoint = binding.endpoint.clone();
        let board_endpoint: message_board::SessionEndpointRef = serde_json::from_value(
            json!({"serviceId":endpoint.service_id,"endpointId":endpoint.endpoint_id}),
        )
        .expect("board endpoint");
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&directory.path().join("operations.sqlite"))
                .await
                .expect("real operation store"),
        ));
        let hub = Arc::new(ProviderSessionEventHub::new(Arc::clone(&store)));
        let mut runtime = tokio::time::timeout(
            Duration::from_secs(5),
            ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
                launch.clone(),
                acp_client_runtime::ProviderModelPicker::Standard,
                "fixture",
                "http://127.0.0.1:1/mcp",
                Arc::clone(&hub),
                board_endpoint.clone(),
            ),
        )
        .await
        .expect("bounded ACP fixture initialization")
        .expect("real runtime and event publication");
        runtime.native_read_context = Some(
            crate::provider_native_capabilities::ProviderNativeReadContext::capture(
                provider, &launch,
            ),
        );
        let endpoint_directory = EndpointDirectory::new(endpoint.service_id.clone());
        endpoint_directory
            .publish(collaboration_protocol::EndpointDescription {
                endpoint: endpoint.clone(),
                label: collaboration_protocol::NonEmptyText::try_from(
                    "Catalogue fixture".to_owned(),
                )
                .expect("label"),
                availability: collaboration_protocol::EndpointAvailability::Available {
                    observed_at: collaboration_protocol::ObservationTimestamp::try_from(
                        "2026-10-24T00:00:00Z".to_owned(),
                    )
                    .expect("timestamp"),
                },
                channels: vec![
                    collaboration_protocol::ChannelDescription::ExternalProvider {
                        transport: binding.transport,
                        binding_id: binding.binding_id.clone(),
                        binding_generation: binding.generation.generation,
                        runtime: binding.runtime.clone(),
                        capabilities: binding.capabilities.clone(),
                    },
                ],
            })
            .expect("ordinary endpoint publication");
        let supervisor = Arc::new(
            ExternalProviderSupervisor::new_with_hub(
                vec![ExternalProviderBinding {
                    identity: binding,
                    runtime,
                }],
                Arc::clone(&store),
                Some(Arc::clone(&hub)),
            )
            .expect("real supervisor"),
        );
        let delivery = Arc::new(ProviderAcpDeliveryRoute::new(
            endpoint.service_id.clone(),
            std::iter::once(endpoint.clone()).collect(),
            endpoint_directory,
            Arc::clone(&supervisor),
            Arc::clone(&store),
            Arc::new(NoLivePeer),
        ));
        let commands = Arc::new(HostSessionCommandPort::new(
            Arc::clone(&supervisor),
            delivery,
            Arc::clone(&hub),
            store,
        ));
        let models = supervisor
            .provider_model_catalog(&endpoint)
            .expect("real model observation");
        let context = RouterSessionAppServerContext::new(
            board_endpoint.clone(),
            serde_json::from_value(json!({"kind":"human","humanId":"catalogue-fixture-owner"}))
                .expect("actor"),
            commands.clone(),
            hub.clone(),
            models,
        )
        .with_native_catalogue(
            supervisor.clone(),
            supervisor.native_catalogue_default_cwd(&endpoint),
        );
        let socket = directory.path().join("catalogue.sock");
        let listener = RouterSessionAppServerListener::bind(&socket, Arc::new(context))
            .expect("real private listener");
        let shutdown = CancellationToken::new();
        let server = Some(tokio::spawn(listener.run(shutdown.clone())));
        Self {
            _directory: directory,
            config,
            workspace,
            export,
            trace,
            native_export_trace,
            hold_prompt,
            socket,
            endpoint,
            board_endpoint,
            supervisor,
            commands,
            hub,
            shutdown,
            server,
        }
    }

    async fn rpc(&self, method: &str) -> Value {
        self.rpc_params(method, json!({"cwds":[self.workspace]}))
            .await
    }

    async fn rpc_params(&self, method: &str, params: Value) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            let stream = tokio::net::UnixStream::connect(&self.socket)
                .await
                .expect("private socket");
            let (mut client, _) = client_async("ws://localhost/rpc", stream)
                .await
                .expect("actual websocket framing");
            client
                .send(Message::Text(
                    json!({"id":73,"method":method,"params":params})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("consumer request");
            let response = loop {
                let frame = client
                    .next()
                    .await
                    .expect("response frame")
                    .expect("valid websocket response");
                let response: Value = serde_json::from_str(frame.to_text().expect("text response"))
                    .expect("JSON response");
                if response.get("id") == Some(&json!(73)) {
                    break response;
                }
            };
            client.close(None).await.expect("owned client close");
            response
        })
        .await
        .expect("bounded catalogue request")
    }

    fn read_request(&self, query: CatalogueQuery) -> CatalogueRead {
        CatalogueRead {
            endpoint: self.board_endpoint.clone(),
            query,
            view: CatalogueView::FilesystemCatalogues,
            refresh_native: false,
            actor: serde_json::from_value(
                json!({"kind":"human","humanId":"catalogue-fixture-owner"}),
            )
            .expect("actor"),
        }
    }

    fn install_package_fixture(&self, malformed: bool) -> PathBuf {
        let marketplace = self._directory.path().join("marketplace.json");
        let package = self._directory.path().join("native-package");
        let manifest = package.join(".claude-plugin/plugin.json");
        write(
            &marketplace,
            r#"{"name":"fixture-market","plugins":[{"name":"fixture-package"}]}"#,
        );
        write(
            &manifest,
            if malformed {
                "{ malformed present manifest"
            } else {
                r#"{"name":"fixture-package"}"#
            },
        );
        write(
            &self.config.join("plugins/known_marketplaces.json"),
            &json!({"fixture-market":{"installLocation":marketplace,"source":{"source":"file"}}})
                .to_string(),
        );
        write(&self.export, &json!([{"id":"fixture-package@fixture-market","version":"1","scope":"user","enabled":true,"installPath":package}]).to_string());
        manifest
    }

    fn assert_no_conversation_command(&self) {
        assert_eq!(
            fs::read_to_string(&self.trace).expect("owned ACP trace"),
            "",
            "cold discovery must not create/load/prompt/steer a native conversation"
        );
    }

    fn assert_no_native_export(&self) {
        assert_eq!(
            fs::read_to_string(&self.native_export_trace).expect("owned native export trace"),
            ""
        );
    }

    async fn finish(&mut self) {
        self.assert_no_conversation_command();
        self.stop().await;
    }

    async fn stop(&mut self) {
        self.shutdown.cancel();
        tokio::time::timeout(
            Duration::from_secs(5),
            self.server.take().expect("server owner"),
        )
        .await
        .expect("bounded listener join")
        .expect("listener task")
        .expect("listener shutdown");
        tokio::time::timeout(Duration::from_secs(15), self.supervisor.shutdown())
            .await
            .expect("bounded owned runtime shutdown")
            .expect("owned provider stopped");
    }
}

impl Drop for CatalogueFixture {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(binding) = self.supervisor.inner.bindings.get(&self.endpoint) {
            binding.runtime.retirement().cancel();
        }
    }
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("fixture source directory");
    fs::write(path, contents).expect("native source fixture");
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn native_capabilities_socket_real_supervisor_retains_healthy_skill_and_malformed_source() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    let empty = fixture.rpc("skills/list").await;
    assert_eq!(empty["result"]["data"][0]["skills"], json!([]));
    assert_eq!(empty["result"]["data"][0]["errors"], json!([]));
    write(
        &fixture.workspace.join(".claude/skills/healthy/SKILL.md"),
        "Native healthy source",
    );
    let broken = fixture.workspace.join(".claude/skills/broken/SKILL.md");
    write(&broken, "---\nname: [broken\n---\n");
    let response = fixture.rpc("skills/list").await;
    assert_eq!(
        response["result"]["data"][0]["skills"][0]["name"],
        "healthy"
    );
    assert_eq!(
        response["result"]["data"][0]["errors"][0]["path"],
        json!(fs::canonicalize(broken).expect("actual candidate file"))
    );
    assert_eq!(
        response["result"]["data"][0]["errors"]
            .as_array()
            .expect("located errors")
            .len(),
        1
    );
    fixture.finish().await;
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn native_capabilities_socket_malformed_package_fails_both_affected_sections() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    write(
        &fixture.workspace.join(".claude/skills/healthy/SKILL.md"),
        "Native ordinary skill",
    );
    let manifest = fixture.install_package_fixture(true);
    let observed = fixture
        .supervisor
        .read_catalogue(fixture.read_request(CatalogueQuery::Workspace {
            cwd: CapabilityPath::new(fixture.workspace.clone()).expect("cwd"),
        }))
        .await;
    let CatalogueResult::Ready { report } = observed else {
        panic!("reader retains located native failure")
    };
    let CatalogueSection::Supported { errors, .. } = report.plugins else {
        panic!("accounted package source")
    };
    assert_eq!(
        errors,
        vec![CatalogueSourceError {
            source: NativeErrorSource::OtherFilesystem {
                actual_path: CapabilityPath::new(manifest).expect("actual source")
            },
            reason: CatalogueReadFailure::InvalidNativeData
        }]
    );
    let plugins = fixture.rpc("plugin/list").await;
    assert_eq!(
        plugins["error"]["message"],
        "native filesystem catalogue cannot be represented completely: all native filesystem candidates are unrepresentable (1 entries)"
    );
    assert!(plugins.get("result").is_none());
    let skills = fixture.rpc("skills/list").await;
    assert_eq!(
        skills["error"]["message"],
        "native filesystem catalogue cannot be represented completely: incomplete native filesystem view: partial native export (1 limitations)"
    );
    assert!(skills.get("result").is_none());
    fixture.finish().await;
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn native_capabilities_socket_skill_setup_failure_does_not_poison_qualified_plugin_rows() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    fixture.install_package_fixture(false);
    write(
        &fixture.workspace.join(".claude/settings.local.json"),
        "{ invalid native settings",
    );
    let plugins = fixture.rpc("plugin/list").await;
    assert_eq!(
        plugins["result"]["marketplaces"][0]["plugins"][0]["name"],
        "fixture-package"
    );
    let skills = fixture.rpc("skills/list").await;
    assert_eq!(
        skills["error"]["message"],
        "native filesystem catalogue cannot be represented completely: incomplete native filesystem view: partial native export (1 limitations)"
    );
    assert!(skills.get("result").is_none());
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_supervisor_foreign_endpoint_and_cursor_home_cannot_read_native_sources()
 {
    let mut fixture = CatalogueFixture::new(ProviderKind::Cursor).await;
    let mut foreign = fixture.read_request(CatalogueQuery::Home);
    foreign.endpoint.endpoint_id =
        message_board::EndpointId::try_from("foreign-provider".to_owned())
            .expect("foreign endpoint");
    assert!(matches!(
        fixture.supervisor.read_catalogue(foreign).await,
        CatalogueResult::Failed {
            reason: CatalogueReadFailure::InvalidScope
        }
    ));
    assert!(matches!(
        fixture
            .supervisor
            .read_catalogue(fixture.read_request(CatalogueQuery::Home))
            .await,
        CatalogueResult::Unsupported {
            reason: CatalogueUnsupported::ScopeUnavailable
        }
    ));
    fixture.assert_no_native_export();
    fixture.finish().await;
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn native_capabilities_supervisor_retirement_cancels_before_native_export() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    fixture
        .supervisor
        .inner
        .bindings
        .get(&fixture.endpoint)
        .expect("actual binding")
        .runtime
        .retirement()
        .cancel();
    let observed = fixture
        .supervisor
        .read_catalogue(fixture.read_request(CatalogueQuery::Workspace {
            cwd: CapabilityPath::new(fixture.workspace.clone()).expect("cwd"),
        }))
        .await;
    assert!(matches!(
        observed,
        CatalogueResult::Failed {
            reason: CatalogueReadFailure::Cancelled
        }
    ));
    fixture.assert_no_native_export();
    fixture.finish().await;
}
