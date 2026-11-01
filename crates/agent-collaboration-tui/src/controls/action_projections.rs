//! Every presentation is freshly derived from the scope's definitions.
use crate::actions::{ActionPresentation, ActionRegistry, Placement};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionProjections {
    pub menu: Vec<ActionPresentation>,
    pub toolbar: Vec<ActionPresentation>,
    pub help: Vec<ActionPresentation>,
    pub hints: Vec<ActionPresentation>,
}
impl ActionProjections {
    pub fn from_registry(registry: &ActionRegistry) -> Self {
        let actions = registry.presentations();
        Self {
            menu: actions
                .iter()
                .filter(|action| action.placement != Placement::Hidden)
                .cloned()
                .collect(),
            toolbar: actions
                .iter()
                .filter(|action| action.placement == Placement::Toolbar)
                .cloned()
                .collect(),
            help: actions.clone(),
            hints: actions,
        }
    }
}

#[cfg(test)]
#[path = "action_controls_test.rs"]
mod action_controls_test;
#[cfg(test)]
#[path = "live_actions_test.rs"]
mod live_actions_test;
