//! Every presentation is freshly derived from the scope's definitions.
use crate::actions::{ActionPresentation, ActionRegistry, MenuSectionSpec, Placement};
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
            menu: project_menu_sections(&actions, registry.menu_sections())
                .into_iter()
                .flat_map(|section| section.items)
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

/// Menu chrome and item order derive together from the action owner's metadata.
pub(crate) struct MenuSectionPresentation {
    pub heading: Option<ActionPresentation>,
    pub items: Vec<ActionPresentation>,
}
pub(crate) fn menu_sections(registry: &ActionRegistry) -> Vec<MenuSectionPresentation> {
    project_menu_sections(&registry.presentations(), registry.menu_sections())
}
fn project_menu_sections(
    actions: &[ActionPresentation],
    specs: &[MenuSectionSpec],
) -> Vec<MenuSectionPresentation> {
    let heading_ids = specs
        .iter()
        .filter_map(|spec| spec.heading_action.as_ref())
        .collect::<std::collections::HashSet<_>>();
    let mut result = Vec::new();
    let toolbar = actions
        .iter()
        .filter(|action| {
            action.placement == Placement::Toolbar && !heading_ids.contains(&action.id)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !toolbar.is_empty() {
        result.push(MenuSectionPresentation {
            heading: None,
            items: toolbar,
        });
    }
    let mut names = specs.iter().map(|spec| spec.section).collect::<Vec<_>>();
    for action in actions {
        if let Placement::Menu { section } = &action.placement
            && !names.contains(section)
        {
            names.push(*section);
        }
    }
    for name in names {
        let items = actions
            .iter()
            .filter(|action| {
                matches!(action.placement,Placement::Menu{section} if section==name)
                    && !heading_ids.contains(&action.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        if items.is_empty() {
            continue;
        }
        let heading = specs
            .iter()
            .find(|spec| spec.section == name)
            .and_then(|spec| spec.heading_action.as_ref())
            .and_then(|id| actions.iter().find(|action| action.id == *id))
            .cloned();
        result.push(MenuSectionPresentation { heading, items });
    }
    result
}

#[cfg(test)]
#[path = "action_controls_test.rs"]
mod action_controls_test;
#[cfg(test)]
#[path = "live_actions_test.rs"]
mod live_actions_test;
