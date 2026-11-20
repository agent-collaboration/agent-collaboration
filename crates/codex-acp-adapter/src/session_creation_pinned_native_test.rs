//! Real pinned app-server policy restoration. History is checkpointed without
//! submitting a model turn or connecting to a live provider.
use super::*;
use crate::{
    ScratchScopeRegister,
    test_support::conversation_operation_recorder::AcceptingConversationRecorder,
};
use std::{os::unix::fs::DirBuilderExt as _, process::Stdio, time::Duration};
use tokio::process::{Child, Command};

#[derive(Default)]
struct ScratchUses(std::sync::Mutex<Vec<PathBuf>>);

impl ScratchScopeRegister for ScratchUses {
    fn opened(&self, folder: &Path) {
        self.0.lock().unwrap().push(folder.to_path_buf());
    }

    fn closed(&self, folder: &Path) {
        let mut opened = self.0.lock().unwrap();
        let index = opened.iter().position(|path| path == folder).unwrap();
        opened.remove(index);
    }
}

async fn start_pinned_server(executable: &Path, home: &Path, socket: &Path) -> Child {
    let mut process = Command::new(executable);
    for setting in codex_native_integration::router_permission_profile_overrides() {
        process.args(["-c", &setting]);
    }
    let child = process
        .args(["app-server", "--listen"])
        .arg(format!("unix://{}", socket.display()))
        .env("CODEX_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("start isolated native fixture");
    tokio::time::timeout(Duration::from_secs(10), async {
        while !socket.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native fixture publishes the Unix socket");
    child
}

fn observation(binding: &AcpSessionBinding) -> (&Value, &Value) {
    match &binding.settings_observation {
        SettingsObservation::Observed {
            native_sandbox: Some(sandbox),
            permission_profile: Some(profile),
            ..
        } => (sandbox, profile),
        other => panic!("expected pinned native settings: {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires an explicitly supplied Codex 0.166.0 binary and isolated fixture parent"]
async fn managed_cold_reopen_keeps_the_named_mode_and_network() {
    let executable =
        PathBuf::from(std::env::var_os("CODEX_ROUTER_I7_NATIVE").expect("pinned Codex binary"));
    assert_eq!(
        codex_native_integration::managed_executable_version(&executable)
            .await
            .unwrap(),
        "0.166.0",
    );
    let parent = PathBuf::from(
        std::env::var_os("CODEX_ROUTER_I7_FIXTURE_PARENT").expect("isolated fixture parent"),
    );
    let root = parent.join(format!("i7-{}", std::process::id()));
    let home = root.join("native-home");
    let project = root.join("project");
    for directory in [&root, &home, &project] {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(directory)
            .unwrap();
    }
    std::fs::write(
        home.join("config.toml"),
        r#"
model_provider = "fixture"
model = "fixture-model"
approval_policy = "never"
[model_providers.fixture]
name = "isolated fixture"
base_url = "http://127.0.0.1:1/v1"
wire_api = "responses"
requires_openai_auth = false
"#,
    )
    .unwrap();
    let identity = codex_native_integration::executable_identity(&executable)
        .await
        .unwrap();
    let export =
        codex_native_integration::NativeSchemaExport::generate(&identity, &root.join("schema"))
            .await
            .unwrap();
    let schemas = Arc::new(NativePayloadSchemas::from_bundle(export.bundle()).unwrap());
    let generation: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch":"00000000-0000-7000-8000-000000000001","generation":1
    }))
    .unwrap();
    let actor = json!({
        "endpoint":{"serviceId":"00000000-0000-7000-8000-000000000002","endpointId":"codex-local"},
        "sessionId":"fixture-actor"
    });
    for (access, profile_id, parent_profile) in [
        ("write-restricted", "router-write-restricted", ":read-only"),
        ("workspace-write", "router-workspace-write", ":workspace"),
    ] {
        let scratch_scope = format!(
            "session-{}",
            collaboration_protocol::OperationId::generate().as_str()
        );
        let scratch = root.join("scratch").join(&scratch_scope);
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
        let scratch_uses = Arc::new(ScratchUses::default());
        let start_socket = root.join(format!("start-{access}.sock"));
        let resume_socket = root.join(format!("resume-{access}.sock"));
        let mut child = start_pinned_server(&executable, &home, &start_socket).await;
        let mut binding = AcpSessionBinding::create(
            &mut AcpSchemaCatalog::load().unwrap(),
            SessionSetupInputs {
                connection: NativeProtocolConnection::connect(&start_socket).await.unwrap(),
                schemas: Arc::clone(&schemas),
                generation: generation.clone(),
                params: json!({"cwd":project,"mcpServers":[],"_meta":{"codexRouter":{
                    "model":"fixture-model","effort":"medium","access":access,
                    "createdBy":actor,"approver":actor,"scratchScope":scratch_scope,"scratchPath":scratch
                }}}),
                operation_id: None,
                recorder: Arc::new(AcceptingConversationRecorder),
                scratch_scopes: scratch_uses.clone(),
            },
        ).await.unwrap();
        let session_id = binding.session_id().to_owned();
        let (start_sandbox, start_profile) = observation(&binding);
        let (start_sandbox, start_profile) = (start_sandbox.clone(), start_profile.clone());
        assert_eq!(start_profile["id"], profile_id);
        assert_eq!(start_profile["extends"], parent_profile);
        assert_eq!(start_sandbox["networkAccess"], true);
        assert_eq!(
            &*scratch_uses.0.lock().unwrap(),
            std::slice::from_ref(&scratch)
        );
        // The acknowledged checkpoint makes the thread restartable without an AI turn.
        binding.connection_mut().request("thread/inject_items", json!({
            "threadId":session_id,
            "items":[{"type":"message","role":"user","content":[{"type":"input_text","text":"isolated fixture"}]}]
        })).await.unwrap();
        drop(binding);
        assert!(scratch_uses.0.lock().unwrap().is_empty());
        child.kill().await.unwrap();
        std::fs::remove_dir(&scratch).unwrap();
        let mut child = start_pinned_server(&executable, &home, &resume_socket).await;
        let (binding, _) = AcpSessionBinding::load_existing(
            &mut AcpSchemaCatalog::load().unwrap(),
            SessionSetupInputs {
                connection: NativeProtocolConnection::connect(&resume_socket)
                    .await
                    .unwrap(),
                schemas: Arc::clone(&schemas),
                generation: generation.clone(),
                params: json!({"sessionId":session_id,"cwd":project,"mcpServers":[]}),
                operation_id: None,
                recorder: Arc::new(AcceptingConversationRecorder),
                scratch_scopes: scratch_uses.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(binding.session_id(), session_id);
        let (resume_sandbox, resume_profile) = observation(&binding);
        assert_eq!(
            resume_profile, &start_profile,
            "{access}: named permission mode"
        );
        assert_eq!(
            resume_sandbox["networkAccess"], start_sandbox["networkAccess"],
            "{access}: network"
        );
        assert_ne!(resume_sandbox["type"], "dangerFullAccess");
        // Production switch gate: session records must restore the private scratch grant.
        assert!(!scratch.exists());
        assert!(scratch_uses.0.lock().unwrap().is_empty());
        assert!(
            !resume_sandbox
                .to_string()
                .contains(scratch.to_str().unwrap())
        );
        if access == "workspace-write" {
            assert_eq!(resume_sandbox["type"], "workspaceWrite");
            assert_eq!(start_sandbox["excludeTmpdirEnvVar"], true);
            assert_eq!(start_sandbox["excludeSlashTmp"], true);
            // Production switch gate: session records must restore the original temp rules.
            assert_eq!(resume_sandbox["excludeTmpdirEnvVar"], false);
            assert_eq!(resume_sandbox["excludeSlashTmp"], false);
        } else {
            assert_eq!(resume_sandbox["type"], "readOnly");
        }
        println!("{access}: create={start_sandbox} cold={resume_sandbox} profile={resume_profile}");
        drop(binding);
        child.kill().await.unwrap();
    }
}
