//! An input's original content and explicit skill selections, carried together.

use crate::{PromptContent, SelectedSkill};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSubmission {
    pub content: Vec<PromptContent>,
    pub selected: Vec<SelectedSkill>,
}

impl ProviderSubmission {
    pub fn plain(content: Vec<PromptContent>) -> Self {
        Self {
            content,
            selected: Vec::new(),
        }
    }
}
