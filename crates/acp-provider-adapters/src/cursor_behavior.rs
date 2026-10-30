//! Cursor's advertised command classification and native mention preparation.

use crate::selected_skill_content::{selected_text, token_ranges};
use acp_client_runtime::{
    PreparedCompact, ProviderBehavior, ProviderCommand, ProviderError, ProviderFacts,
    ProviderUpdate,
};
use session_event_model::{
    PromptContent, ProviderSkill, ProviderSkillScope, ProviderSubmission, SkillInvocationName,
    SkillRowError, SkillRowErrorReason, SkillView,
};
use std::collections::{HashMap, HashSet};

pub struct CursorAcpBehavior;

impl ProviderBehavior for CursorAcpBehavior {
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
