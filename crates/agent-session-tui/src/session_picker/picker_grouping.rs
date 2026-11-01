//! The live preference projects one ordered input for list paint and navigation.
use super::{
    ActionDef, PickerSurfaceContext, Placement, SessionsPickerModel, SurfaceResult, action,
};
use crate::session_picker::SessionGroupFacet;
use agent_collaboration_tui::{
    actions::{ActionTitle, Checked},
    controls::list::{ListGroup, ListGroupHeading, ListRows},
};
use chrono::{DateTime, FixedOffset, Utc};
use std::sync::Arc;

pub type SessionGroupingClock = Arc<dyn Fn() -> DateTime<FixedOffset> + Send + Sync>;

pub fn selection_rows(
    model: &SessionsPickerModel,
    facet: SessionGroupFacet,
    now: DateTime<FixedOffset>,
) -> SurfaceResult<ListRows> {
    if facet == SessionGroupFacet::None {
        return Ok(ListRows::Ungrouped(
            (0..model.visible_record_len())
                .filter_map(|index| model.visible_record_at(index))
                .map(|record| super::picker_screen_scope::row_key(&record.identity))
                .collect::<SurfaceResult<Vec<_>>>()?,
        ));
    }
    let groups = model
        .session_groups(facet, now.with_timezone(&Utc), now.offset())
        .into_iter()
        .map(|group| {
            Ok(ListGroup {
                heading: ListGroupHeading {
                    name: group.heading.name,
                    count: group.heading.count,
                    summary: None,
                },
                keys: group
                    .keys
                    .iter()
                    .map(super::picker_screen_scope::row_key)
                    .collect::<SurfaceResult<Vec<_>>>()?,
            })
        })
        .collect::<SurfaceResult<Vec<_>>>()?;
    Ok(ListRows::Grouped(groups))
}

pub fn add_group_actions(
    actions: &mut Vec<ActionDef>,
    context: &PickerSurfaceContext,
) -> SurfaceResult<()> {
    let preferences = context.group_preferences;
    actions.push(action(
        "cycle-group",
        "Group by",
        &["F6"],
        Placement::Hidden,
        move |()| {
            let mut preferences = preferences;
            let current = preferences.read().facet();
            let index = SessionGroupFacet::CHOICES
                .iter()
                .position(|facet| *facet == current)
                .unwrap_or(0);
            preferences
                .write()
                .choose(SessionGroupFacet::CHOICES[(index + 1) % SessionGroupFacet::CHOICES.len()]);
        },
    )?);
    for facet in SessionGroupFacet::CHOICES {
        let preferences = context.group_preferences;
        let checked = context.group_preferences;
        let mut choice = action(
            &format!("group-{}", facet.stored_value()),
            ActionTitle::dynamic(move || format!("Group by: {}", facet.label())),
            &[],
            Placement::Menu {
                section: "Group by",
            },
            move |()| {
                let mut preferences = preferences;
                preferences.write().choose(facet);
            },
        )?;
        choice.choice = Some(Checked::new(move || checked.read().facet() == facet));
        actions.push(choice);
    }
    Ok(())
}
