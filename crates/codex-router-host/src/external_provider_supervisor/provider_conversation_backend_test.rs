//! Start and show report the requested mode, the agent's actual mode and why
//! they differ (D1 R3), through the supervisor and a scripted Claude agent.

/// Oracle: R3 — start and show give the requested mode, the agent's actual
/// mode and why they differ; Claude's auto stands in as acceptEdits.
#[tokio::test]
async fn start_and_show_report_requested_and_actual_mode_with_the_reason() {
    use crate::external_provider_supervisor::{
        ExternalProviderBinding, ExternalProviderSupervisor,
    };
    use collaboration_contracts::ProviderConversationBackend;
    use collaboration_protocol::{
        ConversationCreateRequest, ConversationOperationSettlement,
        ConversationOperationWaitOutput, ConversationOperationWaitRequest, OperationId,
        PositiveSeconds, ProviderModeReason, ProviderRequestedPolicy, ProviderRequestedSettings,
        ProviderSessionInspectRequest, ProviderWorkingDirectory, RouterAccess,
    };

    let root = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(root.path()).unwrap();
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let runtime = crate::ExternalProviderRuntime::initialize(
        crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "permission-mode-agent.ts",
            vec![
                base.join("receipt.json").display().to_string(),
                "claude".to_owned(),
                "grants-directories".to_owned(),
            ],
        ),
        std::sync::Arc::new(acp_provider_adapters::ClaudeAcpBehavior::new(
            acp_provider_adapters::ClaudeSkillSource::Annotated,
        )),
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
        Some(base.join("scratch")),
    )
    .unwrap();
    for (requested, actual, reason) in [
        (Some("auto"), "auto", None::<ProviderModeReason>),
        (Some("plan"), "plan", None),
    ] {
        let operation_id = OperationId::generate();
        backend
            .create(ConversationCreateRequest {
                work_roots: Vec::new(),
                settings: requested.map(|mode: &str| ProviderRequestedSettings {
                    mode: Some(mode.to_owned()),
                    ..ProviderRequestedSettings::default()
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
        let ConversationOperationWaitOutput::Available {
            settlement:
                ConversationOperationSettlement::Created {
                    target,
                    effective_settings,
                },
        } = settled.output
        else {
            panic!("{:?}", settled.output);
        };
        assert_eq!(effective_settings.mode.as_deref(), Some(actual));
        assert_eq!(effective_settings.requested_mode.as_deref(), requested);
        assert_eq!(effective_settings.mode_reason, reason);
        let shown = backend
            .inspect_session(ProviderSessionInspectRequest {
                target: target.clone(),
            })
            .await
            .unwrap();
        assert_eq!(
            shown
                .settings_catalog
                .and_then(|catalog| catalog.current_mode)
                .as_deref(),
            Some(actual)
        );
        assert_eq!(shown.requested_mode.as_deref(), requested);
        assert_eq!(shown.mode_reason, reason);
    }
    backend.shutdown().await.unwrap();
}
