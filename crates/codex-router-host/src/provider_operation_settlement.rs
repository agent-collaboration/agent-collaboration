//! Persist applied provider effects and session metadata at one settlement boundary.
use collaboration_protocol::{
    AppliedProviderSetting, ConversationOperationFailure, ConversationOperationSettlement,
    EffectiveProviderSettings, FailedProviderSetting, FastState, MessageText,
    NotAppliedProviderSetting, OperationId, ProviderAuthenticationState, ProviderIdentity,
    ProviderOperationEffect, ProviderPromptStopReason, ProviderReconciliationState,
    ProviderRequestedPolicy, ProviderSettingName, ProviderSettingsMappingStatus,
    ProviderWorkingDirectory, SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderOperationStoreError;
use collaboration_runtime::{ProviderSessionRecord, ProviderSessionResumeInputs};

pub(crate) enum ProviderOperationCompletion {
    Success {
        settlement: ConversationOperationSettlement,
        target: Option<SessionRef>,
        session_record: Option<Box<ProviderSessionRecord>>,
    },
    Failure(ConversationOperationFailure),
    FailureWithSession {
        failure: ConversationOperationFailure,
        session_record: Box<ProviderSessionRecord>,
    },
}

pub(crate) async fn persist_provider_success(
    store: &mut ProviderOperationStore,
    operation_id: &OperationId,
    target: Option<&SessionRef>,
    session_record: Option<&ProviderSessionRecord>,
    terminal_stop_reason: Option<ProviderPromptStopReason>,
) -> Result<(), ProviderOperationStoreError> {
    if let Some(session_record) = session_record {
        return store
            .settle_session_operation(operation_id, session_record)
            .await;
    }
    if let Some(target) = target {
        store
            .record_target(operation_id, target, chrono::Utc::now().timestamp_millis())
            .await?;
    }
    store
        .record_terminal(
            operation_id,
            ProviderOperationEffect::Applied,
            ProviderReconciliationState::Confirmed,
            terminal_stop_reason,
            chrono::Utc::now().timestamp_millis(),
        )
        .await?;
    Ok(())
}

pub(crate) fn effective_settings(
    requested_policy: ProviderRequestedPolicy,
) -> EffectiveProviderSettings {
    EffectiveProviderSettings {
        requested_policy,
        mapping_status: ProviderSettingsMappingStatus::Unverified,
        authentication: ProviderAuthenticationState::Unverified,
        provider_permission_mode: None,
        permission_outcome: None,
        mode: None,
        model: None,
        effort: None,
        requested_mode: None,
        mode_reason: None,
        fast: None,
    }
}

/// What the agent last reported for its fast option, on the wire (D1 R11).
pub(crate) fn provider_fast_state(observed: acp_client_runtime::FastObserved) -> FastState {
    match observed {
        acp_client_runtime::FastObserved::Off => FastState::Off,
        acp_client_runtime::FastObserved::On => FastState::On,
        acp_client_runtime::FastObserved::NoOption => FastState::None,
    }
}

pub(crate) fn provider_setting_name(
    kind: provider_session_machines::ProviderSettingKind,
) -> ProviderSettingName {
    match kind {
        provider_session_machines::ProviderSettingKind::Mode => ProviderSettingName::Mode,
        provider_session_machines::ProviderSettingKind::Model => ProviderSettingName::Model,
        provider_session_machines::ProviderSettingKind::Effort => ProviderSettingName::Effort,
        provider_session_machines::ProviderSettingKind::Fast => ProviderSettingName::Fast,
    }
}

pub(crate) fn provider_applied_setting(
    setting: acp_client_runtime::AppliedProviderSetting,
) -> AppliedProviderSetting {
    AppliedProviderSetting {
        setting: provider_setting_name(setting.kind),
        value: setting.value,
    }
}

pub(crate) fn provider_failed_setting(
    setting: acp_client_runtime::FailedProviderSetting,
) -> FailedProviderSetting {
    FailedProviderSetting {
        setting: provider_setting_name(setting.kind),
        value: setting.value,
        reason: setting.reason,
    }
}

pub(crate) fn provider_not_applied_setting(
    setting: acp_client_runtime::NotAppliedProviderSetting,
) -> NotAppliedProviderSetting {
    NotAppliedProviderSetting {
        setting: provider_setting_name(setting.kind),
        value: setting.value,
    }
}

pub(crate) fn optional_message_text(output: String) -> Result<Option<MessageText>, ()> {
    if output.is_empty() {
        Ok(None)
    } else {
        MessageText::try_from(output).map(Some).map_err(|_| ())
    }
}

pub(crate) fn provider_session_record(
    target: SessionRef,
    working_directory: ProviderWorkingDirectory,
    requested_policy: ProviderRequestedPolicy,
    created_by: ProviderIdentity,
    approver: ProviderIdentity,
    resume: ProviderSessionResumeInputs,
) -> ProviderSessionRecord {
    ProviderSessionRecord {
        target,
        working_directory,
        requested_policy,
        created_by,
        approver,
        updated_at_ms: chrono::Utc::now().timestamp_millis(),
        resume,
    }
}

/// What a create records for a later resume to rebuild (D1 R18; owner
/// Decision 3): the mode it asked for, the work roots its creator named, and
/// the scratch folder the Router made for it.
pub(crate) fn created_session_resume_inputs(
    requested_mode: Option<String>,
    named_work_roots: Vec<ProviderWorkingDirectory>,
    scratch_folder: Option<&std::path::Path>,
) -> ProviderSessionResumeInputs {
    ProviderSessionResumeInputs {
        requested_mode,
        work_roots: Some(named_work_roots),
        scratch_folder: scratch_folder
            .and_then(std::path::Path::to_str)
            .and_then(|folder| ProviderWorkingDirectory::try_from(folder.to_owned()).ok()),
    }
}

pub(crate) const fn provider_mode_reason(
    reason: acp_client_runtime::PermissionModeReason,
) -> collaboration_protocol::ProviderModeReason {
    use acp_client_runtime::PermissionModeReason as Runtime;
    use collaboration_protocol::ProviderModeReason as Protocol;
    match reason {
        Runtime::AutoNotOffered => Protocol::AutoNotOffered,
        Runtime::AgentFellBack => Protocol::AgentFellBack,
        Runtime::AutoNotRealized => Protocol::AutoNotRealized,
        Runtime::EscalationUnproven => Protocol::EscalationUnproven,
        Runtime::AutoNotApplied => Protocol::AutoNotApplied,
    }
}

pub(crate) const fn provider_auto_realization(
    realization: acp_client_runtime::AutoRealization,
) -> collaboration_protocol::ProviderAutoRealization {
    use acp_client_runtime::AutoRealization as Runtime;
    use collaboration_protocol::ProviderAutoRealization as Protocol;
    match realization {
        Runtime::ClassifierEscalates => Protocol::ClassifierEscalates,
        Runtime::AsksFirst => Protocol::AsksFirst,
        Runtime::Unknown => Protocol::Unknown,
    }
}
