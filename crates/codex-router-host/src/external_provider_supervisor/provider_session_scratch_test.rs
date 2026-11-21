use super::prepare_provider_session_scratch;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn each_create_gets_its_own_private_stamped_folder() {
    let root = tempfile::tempdir().unwrap();
    let scratch_root = root.path().join("scratch");
    let first = prepare_provider_session_scratch(Some(&scratch_root))
        .await
        .unwrap()
        .unwrap();
    let second = prepare_provider_session_scratch(Some(&scratch_root))
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
    for folder in [&first, &second] {
        assert_eq!(folder.parent(), Some(scratch_root.as_path()));
        let name = folder.file_name().unwrap().to_str().unwrap();
        let id = name.strip_prefix("session-").unwrap();
        assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
        let mode = std::fs::metadata(folder).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "owner-private");
        assert!(
            folder
                .join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME)
                .is_file(),
            "stamped as used, so the sweep keeps it"
        );
    }
}

#[tokio::test]
async fn a_supervisor_without_a_scratch_root_grants_no_folder() {
    assert_eq!(prepare_provider_session_scratch(None).await, Ok(None));
}

/// Oracle: R4 as corrected in r2 — a Claude or Cursor session created through
/// the supervisor is granted its own new `session-<uuidv7>` folder under the
/// Router's scratch root, and nothing else.
#[tokio::test]
async fn supervisor_create_grants_the_agent_a_fresh_session_folder() {
    use crate::external_provider_supervisor::{
        ExternalProviderBinding, ExternalProviderSupervisor,
    };
    use collaboration_contracts::ProviderConversationBackend;
    use collaboration_protocol::{
        ConversationCreateRequest, ConversationOperationSettlement,
        ConversationOperationWaitOutput, ConversationOperationWaitRequest, OperationId,
        PositiveSeconds, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    };

    let root = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(root.path()).unwrap();
    let scratch_root = base.join("scratch");
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let receipt = base.join("receipt.json");
    let runtime = crate::ExternalProviderRuntime::initialize(
        crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "permission-mode-agent.ts",
            vec![
                receipt.display().to_string(),
                "cursor".to_owned(),
                "grants-directories".to_owned(),
            ],
        ),
        std::sync::Arc::new(acp_provider_adapters::CursorAcpBehavior),
    )
    .await
    .unwrap();
    let store =
        collaboration_runtime::ProviderOperationStore::open(&base.join("operations.sqlite"))
            .await
            .unwrap();
    let binding = crate::external_provider_supervisor::tests::binding();
    let requester = crate::external_provider_supervisor::tests::requester();
    let backend = ExternalProviderSupervisor::new_with_hub(
        vec![ExternalProviderBinding {
            identity: binding.clone(),
            runtime,
        }],
        std::sync::Arc::new(tokio::sync::Mutex::new(store)),
        None,
        Some(scratch_root.clone()),
    )
    .unwrap();
    let mut folders = Vec::new();
    for _ in 0..2 {
        let operation_id = OperationId::generate();
        backend
            .create(ConversationCreateRequest {
                work_roots: Vec::new(),
                settings: Some(collaboration_protocol::ProviderRequestedSettings {
                    mode: Some("auto".into()),
                    ..collaboration_protocol::ProviderRequestedSettings::default()
                }),
                operation_id: operation_id.clone(),
                endpoint: binding.endpoint.clone(),
                generation: None,
                working_directory: ProviderWorkingDirectory::try_from(
                    project.display().to_string(),
                )
                .unwrap(),
                created_by: requester.clone().into(),
                approver: requester.clone().into(),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WorkspaceWrite,
                },
            })
            .await
            .unwrap();
        let settled = backend
            .wait(ConversationOperationWaitRequest {
                operation_id,
                timeout_seconds: PositiveSeconds::try_from(30).unwrap(),
            })
            .await
            .unwrap();
        assert!(
            matches!(
                settled.output,
                ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::Created { .. }
                }
            ),
            "{:?}",
            settled.output
        );
        let sessions: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
        let granted = sessions["sessions"][folders.len()]["additionalDirectories"].clone();
        let granted = granted.as_array().unwrap();
        assert_eq!(granted.len(), 1);
        let folder = std::path::PathBuf::from(granted[0].as_str().unwrap());
        assert_eq!(folder.parent(), Some(scratch_root.as_path()));
        assert!(folder.is_dir());
        assert!(
            folder
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("session-"))
        );
        folders.push(folder);
    }
    assert_ne!(folders[0], folders[1], "each session gets its own folder");
    backend.shutdown().await.unwrap();
}
