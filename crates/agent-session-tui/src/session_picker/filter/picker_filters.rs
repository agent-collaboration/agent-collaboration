use std::path::Path;

use crate::session_picker::screen::picker_model::SessionsPickerRuntimeView;
use crate::session_picker::screen::picker_request::SessionsPickerRequest;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::SessionsProvider;
use agent_session_inventory::sessions::SessionsSort;
use agent_session_inventory::sessions::SessionsSource;
use agent_session_inventory::sessions::normalized_paths_resolve_to_same_location;
use agent_session_inventory::sessions::session_belongs_to_repository;

pub(crate) fn root_matches(
    root: SessionsPickerRoot,
    request: &SessionsPickerRequest,
    record: &SessionPickerRecord,
) -> bool {
    let Some(cwd) = record.normalized_cwd.as_deref().map(Path::new) else {
        return matches!(root, SessionsPickerRoot::Any);
    };
    match root {
        SessionsPickerRoot::Cwd => {
            normalized_paths_resolve_to_same_location(cwd, &request.current_dir)
        }
        SessionsPickerRoot::Repo => session_belongs_to_repository(
            &request.repository_identity,
            record.git_origin_url.as_deref(),
            cwd,
        ),
        SessionsPickerRoot::Any => true,
    }
}

pub(crate) fn provider_matches(
    provider: &SessionsProvider,
    request: &SessionsPickerRequest,
    record: &SessionPickerRecord,
) -> bool {
    match provider {
        SessionsProvider::Any => true,
        SessionsProvider::Current => record.provider == request.current_provider,
        SessionsProvider::ClaudeCode => false,
        SessionsProvider::Id(provider_id) => record.provider.as_ref() == Some(provider_id),
    }
}

pub(crate) fn source_matches(source: SessionsSource, record: &SessionPickerRecord) -> bool {
    if let Some(native_source) = record.native_source {
        use collaboration_client::protocol::NativeSessionSource;
        return matches!(
            (source, native_source),
            (SessionsSource::All, _)
                | (
                    SessionsSource::Interactive,
                    NativeSessionSource::Interactive
                )
                | (SessionsSource::Subagents, NativeSessionSource::Subagents)
        );
    }
    if record.identity.is_provider() {
        return true;
    }
    match source {
        SessionsSource::All => true,
        SessionsSource::Interactive => {
            matches!(record.source.as_deref(), Some("cli" | "vscode"))
                && !matches!(
                    record.thread_source.as_deref(),
                    Some(
                        "system"
                            | "exec"
                            | "app_server"
                            | "subagent"
                            | "guardian_review"
                            | "memory_consolidation"
                    )
                )
        }
        SessionsSource::Subagents => {
            matches!(record.source.as_deref(), Some("subagent"))
                || matches!(
                    record.thread_source.as_deref(),
                    Some("subagent" | "guardian_review" | "memory_consolidation")
                )
                || record
                    .source
                    .as_deref()
                    .and_then(|source| serde_json::from_str::<serde_json::Value>(source).ok())
                    .is_some_and(|source| {
                        source.get("subagent").is_some() || source.get("subAgent").is_some()
                    })
        }
    }
}

pub(crate) fn runtime_view_matches(
    view: SessionsPickerRuntimeView,
    record: &SessionPickerRecord,
) -> bool {
    use agent_session_inventory::picker_runtime_status::PickerRuntimeStatus;

    match view {
        SessionsPickerRuntimeView::Blocked => record.runtime_status == PickerRuntimeStatus::Blocked,
        SessionsPickerRuntimeView::Active => record.runtime_status == PickerRuntimeStatus::Active,
        SessionsPickerRuntimeView::Idle => record.runtime_status == PickerRuntimeStatus::Idle,
        SessionsPickerRuntimeView::All => true,
    }
}

pub(crate) fn next_root_filter(root: SessionsPickerRoot) -> SessionsPickerRoot {
    match root {
        SessionsPickerRoot::Cwd => SessionsPickerRoot::Repo,
        SessionsPickerRoot::Repo => SessionsPickerRoot::Any,
        SessionsPickerRoot::Any => SessionsPickerRoot::Cwd,
    }
}

pub(crate) fn next_sort_filter(sort: SessionsSort) -> SessionsSort {
    match sort {
        SessionsSort::Updated => SessionsSort::Created,
        SessionsSort::Created => SessionsSort::Updated,
    }
}

#[cfg(test)]
mod tests {
    use crate::session_picker::filter::picker_filters::root_matches;
    use crate::session_picker::screen::fixtures::fixture_data::picker_request;
    use agent_session_inventory::picker_data_query::SessionsPickerRoot;

    #[test]
    fn cwd_scope_treats_var_and_private_var_as_the_same_location() {
        let mut request = picker_request();
        request.current_dir = "/private/var/folders/session-work".into();
        let mut record = request.records.remove(0);
        record.normalized_cwd = Some("/var/folders/session-work".to_owned());

        assert!(root_matches(SessionsPickerRoot::Cwd, &request, &record));
    }
}
#[test]
fn native_source_classification_does_not_invent_local_source_metadata() {
    use collaboration_client::protocol::NativeSessionSource;
    let request = crate::session_picker::screen::fixtures::fixture_data::picker_request();
    for (observed, interactive, subagents) in [
        (NativeSessionSource::Interactive, true, false),
        (NativeSessionSource::Subagents, false, true),
        (NativeSessionSource::All, false, false),
    ] {
        let mut record = request.records[0].clone();
        record.native_source = Some(observed);
        record.source = None;
        record.thread_source = None;
        assert!(source_matches(SessionsSource::All, &record));
        assert_eq!(
            source_matches(SessionsSource::Interactive, &record),
            interactive
        );
        assert_eq!(
            source_matches(SessionsSource::Subagents, &record),
            subagents
        );
    }
}
