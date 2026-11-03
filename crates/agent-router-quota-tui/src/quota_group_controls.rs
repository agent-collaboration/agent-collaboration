//! Grouping bindings and menu choices share the committed filter setting.
use super::quota_control_context::{QuotaControlContext, QuotaControlResult, quota_action};
use agent_collaboration_tui::actions::{ActionDef, Checked, Placement};

fn select_group_facet(context: &QuotaControlContext, facet: crate::QuotaGroupFacet) {
    let mut filters = context.filters;
    let mut next = *filters.read();
    next.committed.group_by = facet;
    filters.set(next);
    if let Some(preferences) = &context.group_preferences {
        preferences.remember(facet);
    }
}

pub(crate) fn grouping_actions(
    context: &QuotaControlContext,
) -> QuotaControlResult<Vec<ActionDef>> {
    let cycle_context = context.clone();
    let cycle = quota_action(
        "group-cycle",
        "Group by",
        &["F6"],
        Placement::Hidden,
        move |()| {
            let current = cycle_context.filters.read().committed.group_by;
            select_group_facet(&cycle_context, current.next_facet());
        },
    )?;
    let mut actions = vec![cycle];
    for (name, facet) in [
        ("group-none", crate::QuotaGroupFacet::None),
        ("group-provider", crate::QuotaGroupFacet::Provider),
        ("group-status", crate::QuotaGroupFacet::Status),
    ] {
        let choice_context = context.clone();
        let mut action = quota_action(
            name,
            facet.label(),
            &[],
            Placement::Menu { section: "View" },
            move |()| {
                select_group_facet(&choice_context, facet);
            },
        )?;
        let checked = context.filters;
        action.choice = Some(Checked::new(move || {
            checked.read().committed.group_by == facet
        }));
        actions.push(action);
    }
    Ok(actions)
}
