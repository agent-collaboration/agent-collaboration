//! Decode the bounded direct selection before attachment reads or setting writes.
use super::*;
use crate::{
    SelectedItem, SelectionFailure,
    provider_capabilities::{CapabilityName, CapabilityPath},
};

pub(super) struct DecodedSelection {
    pub content: Vec<CommandContent>,
    pub selections: Vec<SelectedItem>,
}

pub(super) async fn prepare(
    params: &Value,
    target: &SessionRef,
    input_id: &session_event_model::InputId,
    actor: &Identity,
    commands: &dyn SessionCommandPort,
) -> Result<Option<Vec<CommandContent>>, ThreadMethodError> {
    let Some(selected) = decode(params)? else {
        return Ok(None);
    };
    match commands
        .prepare_selected_prompt(crate::SelectedPromptCommand {
            target: target.clone(),
            input_id: input_id.clone(),
            actor: actor.clone(),
            content: selected.content,
            selections: selected.selections,
        })
        .await
        .map_err(|_| ThreadMethodError::Unavailable)?
    {
        crate::SelectedPromptPreparation::Ready { content } => Ok(Some(content)),
        crate::SelectedPromptPreparation::Rejected { reason } => {
            Err(ThreadMethodError::SelectionRejected(reason))
        }
    }
}

pub(super) fn contains_selection(params: &Value) -> bool {
    params
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|input| {
            input.iter().any(|entry| {
                matches!(
                    entry.get("type").and_then(Value::as_str),
                    Some("skill" | "mention")
                )
            })
        })
}

pub(super) fn decode(params: &Value) -> Result<Option<DecodedSelection>, ThreadMethodError> {
    if !contains_selection(params) {
        return Ok(None);
    }
    let rejected = || ThreadMethodError::SelectionRejected(SelectionFailure::UnsupportedForm);
    let input = params
        .get("input")
        .and_then(Value::as_array)
        .ok_or_else(rejected)?;
    let [text, skill] = input.as_slice() else {
        return Err(rejected());
    };
    if text.get("type").and_then(Value::as_str) != Some("text")
        || skill.get("type").and_then(Value::as_str) != Some("skill")
    {
        return Err(rejected());
    }
    let text = text
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(rejected)?;
    if text.len() > collaboration_protocol::MAX_CONTROL_FRAME_BYTES {
        return Err(ThreadMethodError::PromptTooLarge);
    }
    let name = skill
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(rejected)?;
    let path = skill
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(rejected)?;
    Ok(Some(DecodedSelection {
        content: vec![CommandContent::text(text.to_owned()).map_err(|_| rejected())?],
        selections: vec![SelectedItem::Skill {
            submitted_name: CapabilityName::new(name.to_owned()).map_err(|_| rejected())?,
            submitted_path: CapabilityPath::new(PathBuf::from(path)).map_err(|_| rejected())?,
        }],
    }))
}
