//! Cursor's advertised command classification and native mention preparation.

use crate::selected_skill_content::{selected_text, token_ranges};
use acp_client_runtime::{
    PermissionRequestAction, PreparedCompact, ProviderBehavior, ProviderCommand, ProviderError,
    ProviderFacts, ProviderUpdate, StatedPermissionTarget,
};
use session_event_model::{
    PromptContent, ProviderSkill, ProviderSkillScope, ProviderSubmission, SkillInvocationName,
    SkillRowError, SkillRowErrorReason, SkillView,
};
use std::collections::{HashMap, HashSet};

pub struct CursorAcpBehavior;

impl ProviderBehavior for CursorAcpBehavior {
    fn restore_invalid_params_means_session_not_found(
        &self,
        requested_session_id: &str,
        data: &serde_json::Value,
    ) -> bool {
        // Cursor reports a lost session as InvalidParams during restore (UW-17).
        data.get("message")
            .and_then(serde_json::Value::as_str)
            .and_then(|message| message.strip_prefix("Session \""))
            .and_then(|message| message.strip_suffix("\" not found"))
            == Some(requested_session_id)
    }

    fn initialization_authentication(
        &self,
    ) -> acp_client_runtime::ProviderInitializationAuthentication {
        acp_client_runtime::ProviderInitializationAuthentication::RequiredAgentMethod {
            method_id: "cursor_login",
        }
    }
    fn can_reuse_setting_confirmation(
        &self,
        kind: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        // Cursor's shared model manager lets another Session change model/effort.
        kind == provider_session_machines::ProviderSettingKind::Mode
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        acp_client_runtime::ModelMetadataSource::ConnectionRequest {
            method: "cursor/list_available_models",
        }
    }
    fn decode_model_metadata(
        &self,
        value: &serde_json::Value,
    ) -> provider_session_machines::ModelReasoningMetadata {
        crate::model_reasoning_metadata::decode_cursor_models(value)
    }
    fn stated_permission_target(
        &self,
        action: PermissionRequestAction,
        title: &str,
    ) -> Option<StatedPermissionTarget> {
        cursor_stated_permission_target(action, title)
    }
    fn auto_permission_mode(&self) -> acp_client_runtime::AutoPermissionMode {
        // Auto-review is not honoured over ACP (E1), so agent mode asks first and
        // the Router allows in-folder actions.
        acp_client_runtime::AutoPermissionMode {
            realized_by: Some("agent"),
            fallback: None,
            realization: acp_client_runtime::AutoRealization::AsksFirst,
            // Cursor's own auto-review is not offered over ACP (E1), so its
            // ask-first agent mode stands in, and the report says so (R3).
            stand_in_reason: Some(acp_client_runtime::PermissionModeReason::AutoNotOffered),
        }
    }
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        Ok(ProviderFacts {
            skill_view: match update {
                ProviderUpdate::SessionAdmitted | ProviderUpdate::SessionClosed => {
                    Some(SkillView::Unknown)
                }
                ProviderUpdate::Commands { commands, .. } => Some(classify_commands(commands)),
                ProviderUpdate::ToolStatus { .. } => None,
            },
            compaction: None,
        })
    }

    fn prepare(
        &self,
        mut submission: ProviderSubmission,
    ) -> Result<Vec<PromptContent>, ProviderError> {
        if submission.selected.is_empty() {
            return Ok(submission.content);
        }
        let (text_index, text, names) = selected_text(&submission)?;
        let mut replacements = Vec::new();
        for name in names {
            if !invocable_name(name) {
                return Err(ProviderError::InvalidSelection);
            }
            let ranges = token_ranges(text, name);
            if ranges.is_empty() {
                return Err(ProviderError::InvalidSelection);
            }
            replacements.extend(ranges.into_iter().map(|range| (range, name)));
        }
        replacements.sort_by_key(|(range, _)| range.start);
        let mut prepared = String::with_capacity(text.len());
        let mut copied_through = 0;
        for (range, name) in replacements {
            prepared.push_str(
                text.get(copied_through..range.start)
                    .ok_or(ProviderError::InvalidSelection)?,
            );
            prepared.push('/');
            prepared.push_str(name);
            copied_through = range.end;
        }
        prepared.push_str(
            text.get(copied_through..)
                .ok_or(ProviderError::InvalidSelection)?,
        );
        *submission
            .content
            .get_mut(text_index)
            .ok_or(ProviderError::InvalidSelection)? =
            PromptContent::text(prepared).map_err(|_| ProviderError::InvalidSelection)?;
        Ok(submission.content)
    }

    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        Err(ProviderError::Unsupported)
    }
}

fn invocable_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn classify_commands(commands: Vec<ProviderCommand>) -> SkillView {
    let mut counts = HashMap::new();
    for command in &commands {
        *counts
            .entry(command.invocation_name.as_str())
            .or_insert(0usize) += 1;
    }
    let mut seen = HashSet::new();
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    for command in &commands {
        if !seen.insert(command.invocation_name.as_str()) {
            continue;
        }
        let classified = if counts
            .get(command.invocation_name.as_str())
            .copied()
            .unwrap_or(0)
            > 1
        {
            Err(SkillRowErrorReason::AmbiguousName)
        } else {
            classify_command(command)
        };
        match classified {
            Ok(Some(skill)) => skills.push(skill),
            Ok(None) => {}
            Err(reason) => errors.push(SkillRowError {
                advertised_name: command.invocation_name.clone(),
                reason,
            }),
        }
    }
    SkillView::Available { skills, errors }
}

fn classify_command(
    command: &ProviderCommand,
) -> Result<Option<ProviderSkill>, SkillRowErrorReason> {
    let skill = [
        (" (project skill)", ProviderSkillScope::Repo),
        (" (user skill)", ProviderSkillScope::User),
        (" (builtin skill)", ProviderSkillScope::System),
        (" (skill)", ProviderSkillScope::System),
    ]
    .into_iter()
    .find_map(|(suffix, scope)| {
        command
            .description
            .strip_suffix(suffix)
            .map(|description| (description, scope))
    });
    if let Some((description, scope)) = skill {
        if !invocable_name(&command.invocation_name) {
            return Err(SkillRowErrorReason::UninvocableName);
        }
        return Ok(Some(ProviderSkill {
            invocation_name: SkillInvocationName::new(command.invocation_name.clone())
                .map_err(|_| SkillRowErrorReason::MalformedMetadata)?,
            description: description.into(),
            scope,
        }));
    }
    if command.invocation_name == "copy-request-id"
        || [" (team)", " (global)", " (project)", " (user)"]
            .iter()
            .any(|suffix| command.description.ends_with(suffix))
    {
        return Ok(None);
    }
    Err(SkillRowErrorReason::UnrecognizedMetadata)
}

/// Cursor names the target of a permission request only in its title
/// (`formatOperation` in Cursor 2026.10.22's ACP client): ``Delete `<path>```,
/// ``Read `<path>``` or ``Edit `<path>``` for a file, and the command in
/// backticks for a shell request, with any backtick inside escaped. A title
/// with another backtick inside is not read for certain, so it asks.
fn cursor_stated_permission_target(
    action: PermissionRequestAction,
    title: &str,
) -> Option<StatedPermissionTarget> {
    let quoted = |text: &str| {
        text.strip_prefix('`')
            .and_then(|rest| rest.strip_suffix('`'))
            .filter(|inner| !inner.is_empty() && !inner.contains('`'))
            .map(str::to_owned)
    };
    match action {
        PermissionRequestAction::FileAccess => ["Delete ", "Read ", "Edit "]
            .iter()
            .find_map(|verb| title.strip_prefix(verb))
            .and_then(quoted)
            .map(|path| StatedPermissionTarget::File(path.into())),
        PermissionRequestAction::ShellCommand => quoted(title).map(StatedPermissionTarget::Command),
    }
}

#[cfg(test)]
#[path = "cursor_behavior_test.rs"]
mod tests;
