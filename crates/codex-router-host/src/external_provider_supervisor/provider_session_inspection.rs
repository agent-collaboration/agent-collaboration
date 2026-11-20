//! Inspect one durable provider Session with its live connection observations.

use super::ExternalProviderSupervisor;
use collaboration_protocol::{
    ProviderConfigOptionView, ProviderConfigValueView, ProviderHistoryAvailability,
    ProviderInspectFailure, ProviderInspectFailureKind, ProviderSessionInspectRequest,
    ProviderSessionInspectResult, ProviderSessionState, ProviderSettingChoiceView,
    ProviderSettingsCatalogView, SessionRef,
};
use session_event_model::SessionState;

pub(super) async fn inspect(
    supervisor: &ExternalProviderSupervisor,
    request: ProviderSessionInspectRequest,
) -> Result<ProviderSessionInspectResult, ProviderInspectFailure> {
    let target = request.target;
    use provider_session_machines::session_owner::{HistoryAvailability, ObservedSetting};
    let runtime = supervisor
        .runtime_for(&target.endpoint)
        .ok_or_else(|| failure(ProviderInspectFailureKind::Unavailable, &target))?;
    let clock = runtime.owner_clock();
    let record = router_clock::timeout(clock.as_ref(), std::time::Duration::from_secs(10), async {
        supervisor
            .inner
            .store
            .lock()
            .await
            .session_record(&target)
            .await
    })
    .await
    .map_err(|_| failure(ProviderInspectFailureKind::Unavailable, &target))?
    .map_err(|_| failure(ProviderInspectFailureKind::Unavailable, &target))?
    .ok_or_else(|| failure(ProviderInspectFailureKind::NotFound, &target))?;
    let snapshot = supervisor.owner_session_snapshot(&target);
    let capabilities = match snapshot
        .as_ref()
        .and_then(|owner| owner.projection.capabilities.clone())
    {
        Some(report) => report,
        None => runtime
            .capability_report(&String::from(target.session_id.clone()))
            .await
            .to_session_model(),
    };
    let catalog = supervisor.owner_session_catalog(&target);
    let confirmed_mode = snapshot.as_ref().and_then(|owner| {
        match &owner
            .settings
            .get(&provider_session_machines::ProviderSettingKind::Mode)?
            .value
        {
            ObservedSetting::Confirmed(value) => Some(value.0.clone()),
            ObservedSetting::LastConfirmed(_) | ObservedSetting::Unknown => None,
        }
    });
    let fast = snapshot
        .as_ref()
        .and_then(|owner| super::provider_settings_control::snapshot_settings(owner).fast)
        .map(crate::provider_operation_settlement::provider_fast_state);
    let state = snapshot.as_ref().map_or(SessionState::Unloaded, |owner| {
        owner.projection.state.clone()
    });
    let history = if snapshot
        .as_ref()
        .is_some_and(|owner| owner.history == HistoryAvailability::Available)
    {
        ProviderHistoryAvailability::Available
    } else {
        ProviderHistoryAvailability::HistoryUnavailable
    };
    let requested_mode = record.resume.requested_mode.or_else(|| {
        runtime
            .session_setup()
            .requested_mode(&String::from(target.session_id.clone()))
            .requested_mode
    });
    Ok(ProviderSessionInspectResult {
        target,
        state: state_tag(state),
        capabilities,
        history,
        settings_catalog: catalog.map(|catalog| catalog_view(catalog, confirmed_mode)),
        requested_mode,
        mode_reason: snapshot
            .as_ref()
            .and_then(|owner| owner.mode_reason)
            .map(crate::provider_operation_settlement::provider_mode_reason),
        fast,
    })
}

fn catalog_view(
    catalog: acp_client_runtime::ProviderSettingsCatalog,
    current_mode: Option<String>,
) -> ProviderSettingsCatalogView {
    ProviderSettingsCatalogView {
        current_mode,
        modes: catalog.modes.into_iter().map(choice_view).collect(),
        config_options: catalog
            .config_options
            .into_iter()
            .map(|option| ProviderConfigOptionView {
                id: option.id,
                name: option.name,
                category: option
                    .category
                    .map(crate::provider_operation_settlement::provider_setting_name),
                current_value: match option.current_value {
                    acp_client_runtime::ProviderConfigValue::Select(value) => {
                        ProviderConfigValueView::Select { value }
                    }
                    acp_client_runtime::ProviderConfigValue::Boolean(value) => {
                        ProviderConfigValueView::Boolean { value }
                    }
                },
                choices: option.choices.into_iter().map(choice_view).collect(),
            })
            .collect(),
    }
}

fn choice_view(choice: acp_client_runtime::ProviderSettingChoice) -> ProviderSettingChoiceView {
    ProviderSettingChoiceView {
        value: choice.value,
        label: choice.label,
    }
}

fn state_tag(state: SessionState) -> ProviderSessionState {
    match state {
        SessionState::Unloaded => ProviderSessionState::Unloaded,
        SessionState::Idle => ProviderSessionState::Idle,
        SessionState::Running => ProviderSessionState::Running,
        SessionState::RequiresAction { .. } => ProviderSessionState::RequiresAction,
        SessionState::AuthenticationRequired => ProviderSessionState::AuthenticationRequired,
        SessionState::Closed => ProviderSessionState::Closed,
    }
}

fn failure(kind: ProviderInspectFailureKind, target: &SessionRef) -> ProviderInspectFailure {
    ProviderInspectFailure {
        kind,
        stage: None,
        target: Some(target.clone()),
        message: match kind {
            ProviderInspectFailureKind::NotFound => "provider Session is unknown".into(),
            ProviderInspectFailureKind::Unavailable => {
                "provider Session inspection is unavailable".into()
            }
            ProviderInspectFailureKind::Overloaded => {
                "provider Session inspection is overloaded".into()
            }
        },
    }
}

impl ExternalProviderSupervisor {
    pub async fn session_model_catalog(
        &self,
        target: &SessionRef,
    ) -> Result<
        Vec<collaboration_runtime::ProviderModelEntry>,
        collaboration_runtime::SessionModelCatalogError,
    > {
        use collaboration_runtime::SessionModelCatalogError;
        if self
            .inner
            .store
            .lock()
            .await
            .session_record(target)
            .await
            .map_err(|_| SessionModelCatalogError::Unavailable)?
            .is_none()
        {
            return Err(SessionModelCatalogError::SessionNotFound);
        }
        let catalog = match self.owner_session_catalog(target) {
            Some(catalog) => catalog,
            None => self
                .inner
                .settings_catalogs
                .lock()
                .await
                .get(target)
                .cloned()
                .ok_or(SessionModelCatalogError::Unavailable)?,
        };
        Ok(crate::provider_model_catalog::model_entries(&catalog))
    }
}
