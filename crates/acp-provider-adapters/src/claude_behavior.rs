//! Qualified Claude command metadata and final-Text invocation preparation.

use crate::selected_skill_content::{selected_text, token_ranges};
use acp_client_runtime::{
    CompactionTrigger, PreparedCompact, ProviderBehavior, ProviderCommand, ProviderCompactionHint,
    ProviderError, ProviderFacts, ProviderUpdate,
};
use session_event_model::{
    PromptContent, ProviderSkill, ProviderSkillScope, ProviderSubmission, SkillInvocationName,
    SkillRowError, SkillRowErrorReason, SkillSourceError, SkillView,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub enum ClaudeSkillSource {
    Annotated,
    Unavailable(SkillSourceError),
}

pub struct ClaudeAcpBehavior {
    source: ClaudeSkillSource,
}
impl ClaudeAcpBehavior {
    pub fn new(source: ClaudeSkillSource) -> Self {
        Self { source }
    }
}

impl ProviderBehavior for ClaudeAcpBehavior {
    fn can_reuse_setting_confirmation(
        &self,
        _kind: acp_client_runtime::ProviderSettingKind,
    ) -> bool {
        // Only the qualified stock integration establishes per-query settings scope.
        matches!(self.source, ClaudeSkillSource::Annotated)
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        acp_client_runtime::ModelMetadataSource::SessionMetadata
    }
    fn decode_model_metadata(
        &self,
        value: &serde_json::Value,
    ) -> acp_client_runtime::ModelReasoningMetadata {
        crate::model_reasoning_metadata::decode_claude_models(value)
    }
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        if let ClaudeSkillSource::Unavailable(reason) = &self.source {
            return Ok(ProviderFacts {
                skill_view: match update {
                    ProviderUpdate::SessionAdmitted | ProviderUpdate::Commands { .. } => {
                        Some(SkillView::Unavailable(reason.clone()))
                    }
                    ProviderUpdate::SessionClosed => Some(SkillView::Unknown),
                    ProviderUpdate::ToolStatus { .. } => None,
                },
                compaction: None,
            });
        }
        let mut facts = ProviderFacts::default();
        match update {
            ProviderUpdate::SessionAdmitted | ProviderUpdate::SessionClosed => {
                facts.skill_view = Some(SkillView::Unknown)
            }
            ProviderUpdate::Commands { commands, metadata } => {
                facts.skill_view = Some(classify_commands(commands, metadata.as_ref()))
            }
            ProviderUpdate::ToolStatus {
                tool_id,
                status,
                metadata,
            } => {
                if let Some(marker) = metadata
                    .as_ref()
                    .and_then(|meta| meta.get("contextCompaction"))
                {
                    if marker.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
                        return Err(ProviderError::MalformedMetadata);
                    }
                    let trigger = match marker.get("trigger") {
                        None => CompactionTrigger::Unlabelled,
                        Some(serde_json::Value::String(value)) if value == "manual" => {
                            CompactionTrigger::Manual
                        }
                        Some(serde_json::Value::String(value)) if value == "automatic" => {
                            CompactionTrigger::Automatic
                        }
                        Some(_) => return Err(ProviderError::MalformedMetadata),
                    };
                    facts.compaction = Some(ProviderCompactionHint {
                        tool_id,
                        status,
                        trigger,
                    });
                }
            }
        }
        Ok(facts)
    }

    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        if submission.selected.is_empty() {
            return Ok(submission.content);
        }
        if matches!(self.source, ClaudeSkillSource::Unavailable(_)) {
            return Err(ProviderError::SourceUnavailable);
        }
        let (text_index, text, names) = selected_text(&submission)?;
        if names.len() != 1 {
            return Err(ProviderError::Unsupported);
        }
        let name = names
            .into_iter()
            .next()
            .ok_or(ProviderError::InvalidSelection)?;
        let token = token_ranges(text, name)
            .into_iter()
            .next_back()
            .ok_or(ProviderError::InvalidSelection)?;
        let prefix = text
            .get(..token.start)
            .ok_or(ProviderError::InvalidSelection)?;
        let suffix = text
            .get(token.end..)
            .ok_or(ProviderError::InvalidSelection)?;
        let command = PromptContent::text(format!("/{name}{suffix}"))
            .map_err(|_| ProviderError::InvalidSelection)?;
        let prefix = if prefix.is_empty() {
            None
        } else {
            Some(PromptContent::text(prefix.into()).map_err(|_| ProviderError::InvalidSelection)?)
        };
        let mut prepared = Vec::with_capacity(submission.content.len() + 1);
        for (index, block) in submission.content.into_iter().enumerate() {
            if index == text_index {
                prepared.extend(prefix.clone());
            } else {
                prepared.push(block);
            }
        }
        prepared.push(command);
        Ok(prepared)
    }

    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        if matches!(self.source, ClaudeSkillSource::Unavailable(_)) {
            return Err(ProviderError::Unsupported);
        }
        Ok(PreparedCompact::Prompt(vec![
            PromptContent::text("/compact".into()).map_err(|_| ProviderError::Unsupported)?,
        ]))
    }
}

fn classify_commands(
    commands: Vec<ProviderCommand>,
    metadata: Option<&serde_json::Value>,
) -> SkillView {
    let annotation = match router_annotation(metadata) {
        Ok(annotation) => annotation,
        Err(()) => return SkillView::Unavailable(SkillSourceError::MalformedMetadata),
    };
    match annotation.and_then(|meta| meta.get("skillStatus")) {
        None => return SkillView::Unknown,
        Some(serde_json::Value::String(status)) if status == "available" => {}
        Some(serde_json::Value::String(status)) if status == "unavailable" => {
            return SkillView::Unavailable(SkillSourceError::ProviderUnavailable);
        }
        Some(_) => return SkillView::Unavailable(SkillSourceError::MalformedMetadata),
    }
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
        let kind = router_annotation(command.metadata.as_ref())
            .map(|annotation| annotation.and_then(|meta| meta.get("kind")));
        let reason = if counts
            .get(command.invocation_name.as_str())
            .copied()
            .unwrap_or(0)
            > 1
        {
            Some(SkillRowErrorReason::AmbiguousName)
        } else {
            match kind {
                Err(()) => Some(SkillRowErrorReason::MalformedMetadata),
                Ok(None) => continue,
                Ok(Some(serde_json::Value::String(kind))) if kind == "skill" => None,
                Ok(Some(serde_json::Value::String(_))) => {
                    Some(SkillRowErrorReason::UnrecognizedMetadata)
                }
                Ok(Some(_)) => Some(SkillRowErrorReason::MalformedMetadata),
            }
        };
        if let Some(reason) = reason {
            errors.push(SkillRowError {
                advertised_name: command.invocation_name.clone(),
                reason,
            });
            continue;
        }
        match SkillInvocationName::new(command.invocation_name.clone()) {
            Ok(invocation_name) => skills.push(ProviderSkill {
                invocation_name,
                description: command.description.clone(),
                scope: ProviderSkillScope::System,
            }),
            Err(_) => errors.push(SkillRowError {
                advertised_name: command.invocation_name.clone(),
                reason: SkillRowErrorReason::MalformedMetadata,
            }),
        }
    }
    SkillView::Available { skills, errors }
}

fn router_annotation(
    metadata: Option<&serde_json::Value>,
) -> Result<Option<&serde_json::Map<String, serde_json::Value>>, ()> {
    match metadata {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Object(metadata)) => match metadata.get("codexRouter") {
            None => Ok(None),
            Some(serde_json::Value::Object(annotation)) => Ok(Some(annotation)),
            Some(_) => Err(()),
        },
        Some(_) => Err(()),
    }
}

#[cfg(test)]
#[path = "claude_behavior_test.rs"]
mod tests;
