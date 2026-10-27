//! Real SQLite, peer registry, bounded job child and served MCP/client.
#[path = "../../../codex-router-test-support/src/node_fixture.rs"]
mod node_fixture;
use super::served_api::ServedApi;
use claude_code_peer_messaging::{ClaudeCodeSessionRegistry, NativeClaudeJobReader};
use collaboration_protocol::{
    EndpointDescription, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionEventHub;
use collaboration_runtime::ProviderSessionRecord;
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

pub(super) const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
pub(super) const EPOCH: &str = "00000000-0000-4000-8000-000000000002";
pub(super) const SESSION: &str = "00000000-0000-4000-8000-000000000042";
pub(super) type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub(super) fn job(id: &str, session: &str, state: &str) -> Value {
    json!({"kind":"background","id":id,"sessionId":session,"cwd":"/source/native","name":"Native job before prompt","state":state,"startedAt":1790162100000_i64})
}

pub(super) struct CatalogueFixture {
    pub api: ServedApi,
    pub client: collaboration_client::CollaborationClient,
    pub target: SessionRef,
    pub output: PathBuf,
    pub executable: PathBuf,
    pub peers: PathBuf,
    _root: tempfile::TempDir,
}

impl CatalogueFixture {
    pub async fn start(peer_available: bool) -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let peers = root.path().join("peers");
        if peer_available {
            std::fs::create_dir(&peers)?;
        } else {
            std::fs::write(&peers, "owned unavailable registry fixture")?;
        }
        let output = root.path().join("job-output.json");
        std::fs::write(&output, "[]")?;
        let executable = root.path().join("claude");
        node_fixture::write_node_fixture(
            &executable,
            "native-cli-catalogue-agent.ts",
            &json!({"kind":"file","output":{"kind":"path","path":output}}),
        )?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        let description: EndpointDescription = serde_json::from_value(
            json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"label":"Native Claude independent of ACP","availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"No ACP provider"},"channels":[]}),
        )?;
        let target: SessionRef =
            serde_json::from_value(json!({"endpoint":description.endpoint,"sessionId":SESSION}))?;
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&root.path().join("operations.sqlite")).await?,
        ));
        let approver = serde_json::from_value(json!({"humanId":"saved-managed-approver"}))?;
        store
            .lock()
            .await
            .record_session(&ProviderSessionRecord {
                target: target.clone(),
                working_directory: ProviderWorkingDirectory::try_from("/managed/saved".to_owned())?,
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: approver,
                approver: serde_json::from_value(json!({"humanId":"saved-managed-approver"}))?,
                updated_at_ms: 1790162100000_i64,
            })
            .await?;
        let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
        hub.publish(
            serde_json::from_value(serde_json::to_value(&target)?)?,
            session_event_model::SessionEvent::TurnStarted {
                turn_id: "owned-fixture-turn".into(),
                input_id: session_event_model::InputId::new("owned-fixture-input")?,
            },
        )
        .await?;
        let identity = ApplicationFixture::new(SERVICE, EPOCH)?
            .with_endpoints(vec![description])?
            .with_provider_operation_store(store)
            .with_provider_session_hub(hub)
            .with_claude_code_sessions(Arc::new(ClaudeCodeSessionRegistry::new(peers.clone())))
            .with_claude_background_jobs(Arc::new(NativeClaudeJobReader::for_executable(
                executable.clone(),
            )));
        let api = ServedApi::start(identity)?;
        let client = api.client("native-catalogue-fixture").await?;
        Ok(Self {
            api,
            client,
            target,
            output,
            executable,
            peers,
            _root: root,
        })
    }
    pub fn write_jobs(&self, jobs: Vec<Value>) -> TestResult<()> {
        std::fs::write(&self.output, serde_json::to_vec(&jobs)?)?;
        Ok(())
    }
    pub fn write_peer(&self, session_id: &str, directory: &str, status: &str) -> TestResult<()> {
        let process_id = std::process::id();
        std::fs::write(
            self.peers.join(format!("{process_id}.json")),
            serde_json::to_vec(&json!({
                "pid":process_id,"sessionId":session_id,"cwd":directory,"name":"Observed peer","status":status,"startedAt":1790162100000_i64,"updatedAt":1790162100000_i64,
                "statusUpdatedAt":1790162100000_i64,"kind":"interactive","entrypoint":"cli","peerProtocol":1,"messagingSocketPath":self.peers.join("private-peer.sock")
            }))?,
        )?;
        Ok(())
    }
}
