//! Exact token locations shared by the two providers' content preparation rules.

use acp_client_runtime::ProviderError;
use session_event_model::{PromptContent, ProviderSubmission};
use std::{collections::BTreeSet, ops::Range};

pub(crate) fn selected_text(
    submission: &ProviderSubmission,
) -> Result<(usize, &str, BTreeSet<&str>), ProviderError> {
    let mut text = None;
    for (index, block) in submission.content.iter().enumerate() {
        match block {
            PromptContent::Text { text: value } if text.is_none() => {
                text = Some((index, value.as_str()))
            }
            PromptContent::Image { .. } => {}
            _ => return Err(ProviderError::Unsupported),
        }
    }
    let (index, text) = text.ok_or(ProviderError::Unsupported)?;
    let names = submission
        .selected
        .iter()
        .map(|selection| selection.invocation_name.as_str())
        .collect();
    Ok((index, text, names))
}

pub(crate) fn token_ranges(text: &str, name: &str) -> Vec<Range<usize>> {
    let token = format!("${name}");
    text.match_indices(&token)
        .filter_map(|(start, token)| {
            let end = start + token.len();
            let before = text.get(..start)?.chars().next_back();
            let after = text.get(end..)?.chars().next();
            (before.is_none_or(char::is_whitespace) && after.is_none_or(char::is_whitespace))
                .then_some(start..end)
        })
        .collect()
}
