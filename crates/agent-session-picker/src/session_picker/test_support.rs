use crate::session_picker::picker_request::SessionsPickerRequest;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use agent_session_inventory::picker_runtime_status::PickerRuntimeStatus;
use agent_session_inventory::sessions::RepositoryIdentity;
use agent_session_inventory::sessions::SessionConversationPreview;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::SessionsProvider;
use agent_session_inventory::sessions::SessionsSort;
use agent_session_inventory::sessions::SessionsSource;

#[cfg(test)]
pub fn observed_records(
    records: Vec<SessionPickerRecord>,
) -> agent_session_inventory::picker_runtime_status::PickerRecordsSnapshot {
    agent_session_inventory::picker_runtime_status::PickerRecordsSnapshot {
        native_live_coverage: Vec::new(),
        records,
        runtime_coverage:
            agent_session_inventory::picker_runtime_status::PickerRuntimeCoverage::Available,
    }
}

pub fn picker_request() -> SessionsPickerRequest {
    SessionsPickerRequest {
        root: SessionsPickerRoot::Cwd,
        provider: SessionsProvider::Any,
        source: SessionsSource::Interactive,
        sort: SessionsSort::Updated,
        current_dir: "/repo/project-a".into(),
        repository_identity: RepositoryIdentity {
            normalized_origin: Some("github.com/shravan-agent/codex-router".to_owned()),
            live_roots: vec!["/repo/project-a".into(), "/repo/project-b".into()],
            repository_basename: "project-a".to_owned(),
            fallback_cwd: None,
        },
        current_provider: Some("codex-router".to_owned()),
        new_session_args_display: String::new(),
        native_arguments: Vec::new(),
        include_empty_sessions: false,
        router_registry: agent_session_inventory::sessions::RouterRegistryRead::Missing,
        machine_mode: super::PickerMachineSourceMode::HostedDefault,
        records: vec![
            picker_record(
                "thread-a",
                "Feature design session",
                "/repo/project-a",
                "codex-router",
                "cli",
            ),
            picker_record(
                "thread-b",
                "Provider migration with very very long provider metadata",
                "/repo/project-b",
                "openai-super-long-provider-id-for-width-proof",
                "cli",
            ),
            picker_record(
                "thread-sub",
                "Subagent planning",
                "/repo/project-a",
                "codex-router",
                "subagent",
            ),
        ],
    }
}

pub fn picker_record(
    session_id: &str,
    title: &str,
    cwd: &str,
    provider: &str,
    source: &str,
) -> SessionPickerRecord {
    SessionPickerRecord {
        identity: agent_session_inventory::sessions::SessionPickerIdentity::LocalCodex(
            session_id.to_owned(),
        ),
        provider_family:
            agent_session_inventory::session_provider_family::SessionProviderFamily::Codex,
        source_context: None,
        machine_display_label: None,
        provenance: agent_session_inventory::sessions::SessionRowProvenance::LocalHomeCatalog,
        endpoint_label: None,
        provider_state: None,
        native_background: None,
        native_peer: None,
        native_conflict: None,
        native_inventory_view: None,
        session_id: session_id.to_owned(),
        title: title.to_owned(),
        full_title: title.to_owned(),
        explicit_name: None,
        recency: "now".to_owned(),
        created: "1d ago".to_owned(),
        recency_at_ms: Some(2_000),
        created_at_ms: Some(1_000),
        branch: "main".to_owned(),
        persisted_branch: "main".to_owned(),
        context: cwd.rsplit('/').next().unwrap_or(cwd).to_owned(),
        cwd: Some(cwd.to_owned()),
        normalized_cwd: Some(cwd.to_owned()),
        git_origin_url: Some("https://github.com/shravan-agent/codex-router.git".to_owned()),
        provider: Some(provider.to_owned()),
        model: Some("gpt-5-codex".to_owned()),
        reasoning_effort: Some("low".to_owned()),
        preview: Some(format!("{title} preview text")),
        first_user_message: format!("{title} first real message"),
        conversation: SessionConversationPreview {
            snippets: vec![
                format!("{title} first real message"),
                format!("{title} latest assistant reply"),
            ],
            unavailable_reason: None,
        },
        conversation_source: None,
        source: Some(source.to_owned()),
        thread_source: Some(source.to_owned()),
        native_source: None,
        runtime_status: PickerRuntimeStatus::Unknown,
    }
}

#[cfg(test)]
pub fn picker_action_selection(
    session_id: &str,
) -> agent_session_inventory::sessions::SessionActionSelection {
    let mut record = picker_record(
        session_id,
        "Selected session",
        "/repo/project-a",
        "codex-router",
        "cli",
    );
    record.source_context = Some(super::PickerSourceContext::DefaultHosted);
    agent_session_inventory::sessions::SessionActionSelection::from_picker_record(&record)
}
