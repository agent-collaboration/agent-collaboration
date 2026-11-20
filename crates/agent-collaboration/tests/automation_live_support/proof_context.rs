//! Explicit live-test admission and fresh-thread helpers; fixture tests never call this module.
#[path = "native_notification_diagnostics.rs"]
pub(crate) mod native_notification_diagnostics;
use codex_native_integration::{
    DEFAULT_ROUTER_PORT, NativeOperation, NativePayloadSchemas, NativeProtocolConnection,
    NativeSchemaExport,
};
use collaboration_client::CollaborationClient;
use collaboration_client::protocol::{ChannelDescription, EndpointRef, SessionRef};
use serde_json::{Value, json};
use std::{
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
};
pub type ProofResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

pub struct ProofContext {
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub service_directory: PathBuf,
    pub endpoint: EndpointRef,
    pub native: NativeProtocolConnection,
    pub schemas: Arc<NativePayloadSchemas>,
}
impl ProofContext {
    pub async fn connect() -> ProofResult<Self> {
        let root = PathBuf::from(std::env::var_os("CODEX_AUTOMATION_PROOF_ROOT").ok_or(
            "Set CODEX_AUTOMATION_PROOF_ROOT to a fresh automation-debug-host run directory",
        )?)
        .canonicalize()?;
        if root.parent() != Some(std::path::Path::new("/tmp").canonicalize()?.as_path())
            || std::fs::metadata(&root)?.permissions().mode() & 0o077 != 0
        {
            return Err("Live proof requires a private direct child of /tmp".into());
        }
        let marker: Value =
            serde_json::from_slice(&std::fs::read(root.join("debug-host-context.json"))?)?;
        if marker.get("model").and_then(Value::as_str) != Some("gpt-5.6-luna")
            || marker.get("profile").and_then(Value::as_str) != Some("codex-router-debug")
            || marker
                .get("port")
                .and_then(Value::as_u64)
                .is_none_or(|port| port == u64::from(DEFAULT_ROUTER_PORT) || port == 0)
        {
            return Err("Host did not establish the isolated debug profile and Luna model".into());
        }
        if marker.get("kind").and_then(Value::as_str) == Some("isolatedDeliveryMatrix") {
            let private_home =
                PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?).canonicalize()?;
            let private_codex_home =
                PathBuf::from(std::env::var_os("CODEX_HOME").ok_or("isolated CODEX_HOME missing")?)
                    .canonicalize()?;
            if private_home != root.join("home").canonicalize()?
                || private_codex_home != root.join("codex-home").canonicalize()?
            {
                return Err("Matrix HOME and CODEX_HOME must be inside the private root".into());
            }
        }
        let workspace = root.join("agent-workspace");
        if std::fs::read_dir(&workspace)?.next().is_some() {
            return Err("Live proof requires a previously unused workspace".into());
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("proof-started.json"))?
            .write_all(b"{\"model\":\"gpt-5.6-luna\"}\n")?;
        let service_directory = root.join("agent-communication");
        let client =
            CollaborationClient::connect(&service_directory, "automation-luna-acceptance", "1")
                .await?;
        let inventory = client.list_endpoints().await?;
        let endpoint = inventory
            .endpoints
            .iter()
            .find(|endpoint| String::from(endpoint.endpoint.endpoint_id.clone()) == "codex-local")
            .ok_or("debug native endpoint missing")?;
        let digest = endpoint
            .channels
            .iter()
            .find_map(|channel| match channel {
                ChannelDescription::NativeCodex {
                    generation: Some(_),
                    schema_digest: Some(digest),
                    ..
                } => Some(digest.clone()),
                _ => None,
            })
            .ok_or("debug native generation/schema unavailable")?;
        let endpoint = endpoint.endpoint.clone();
        let codex_home =
            if marker.get("kind").and_then(Value::as_str) == Some("isolatedDeliveryMatrix") {
                root.join("codex-home")
            } else {
                PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?).join(".codex")
            };
        let paths = codex_native_integration::CodexPaths::from_codex_home(codex_home);
        let identity =
            codex_native_integration::executable_identity(&paths.managed_executable()).await?;
        let export =
            NativeSchemaExport::generate(&identity, &root.join("proof-native-schema")).await?;
        let schemas = NativePayloadSchemas::from_bundle(export.bundle())?;
        if schemas.schema_digest() != String::from(digest) {
            return Err("Running endpoint and proof executable expose different schemas".into());
        }
        let native =
            NativeProtocolConnection::connect(&root.join("native-socket/app-server.sock")).await?;
        Ok(Self {
            root,
            workspace,
            service_directory,
            endpoint,
            native,
            schemas: Arc::new(schemas),
        })
    }
    pub async fn start_thread(&mut self, role: &str) -> ProofResult<SessionRef> {
        self.start_thread_with_instructions(
            role,
            format!(
                "You are {role} in an isolated local acceptance test. Follow the supplied test task. Use only the supplied test commands and this fresh workspace. Never inspect credentials, change configuration, access production services or existing user threads. Do not spawn other agents. A message receipt proves acceptance, not completion."
            ),
        )
        .await
    }

    async fn start_thread_with_instructions(
        &mut self,
        role: &str,
        developer_instructions: String,
    ) -> ProofResult<SessionRef> {
        let workspace = self.workspace.clone();
        self.start_thread_with_context(role, developer_instructions, &workspace)
            .await
    }

    async fn start_thread_with_context(
        &mut self,
        role: &str,
        developer_instructions: String,
        working_directory: &std::path::Path,
    ) -> ProofResult<SessionRef> {
        let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let proxy_port = reservation.local_addr()?.port();
        let control_socket = self.service_directory.join("control.sock").canonicalize()?;
        let configuration = json!({
            "permissions.automation-proof": {"extends":":workspace","network":{"enabled":true}},
            "features.hooks": false,
            "features.network_proxy": {
                "enabled":true,"proxy_url":format!("http://127.0.0.1:{proxy_port}"),
                "enable_socks5":false,"allow_upstream_proxy":false,"allow_local_binding":false,
                "credential_broker":false,"domains":{},"unix_sockets":{control_socket.to_string_lossy().as_ref():"allow"}
            }
        });
        drop(reservation);
        let response = self.native.request_validated(&self.schemas, NativeOperation::StartThread, json!({
            "model":"gpt-5.6-luna","allowProviderModelFallback":false,"cwd":working_directory,
            "permissions":"automation-proof","approvalPolicy":"never","config":configuration,
            "developerInstructions":developer_instructions
        })).await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let rejection = self.native.take_last_rejection();
                self.record(
                    "nativeThreadRejected",
                    json!({"role":role,"rejection":rejection}),
                )?;
                return Err(error.into());
            }
        };
        if response.get("model").and_then(Value::as_str) != Some("gpt-5.6-luna")
            || response.get("modelProvider").and_then(Value::as_str) != Some("codex-router-debug")
        {
            return Err(
                "Native thread did not confirm Luna on the debug provider; no turn was submitted"
                    .into(),
            );
        }
        let target = SessionRef {
            endpoint: self.endpoint.clone(),
            session_id: response
                .pointer("/thread/id")
                .and_then(Value::as_str)
                .ok_or("new native thread identity missing")?
                .to_owned()
                .try_into()?,
        };
        self.record(
            "freshThread",
            json!({"role":role,"target":target,"model":"gpt-5.6-luna"}),
        )?;
        Ok(target)
    }
    pub fn record(&self, event: &str, details: Value) -> ProofResult<()> {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(self.root.join("proof-events.jsonl"))?;
        writeln!(file, "{}", json!({"event":event,"details":details}))?;
        println!("Luna acceptance: {event}");
        Ok(())
    }
}
pub fn agent_text(turn: &Value) -> String {
    turn.get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("agentMessage"))
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
