use crate::test_support::core_models::{surface, target};
use agent_collaboration_tui::{
    actions::{ActionId, Availability, AvailabilityFn, ScopeId},
    surfaces::{
        focus_model::{FocusDirection, FocusModel, GroupId, TargetId},
        surface_stack::SurfaceStack,
    },
};
#[test]
fn removed_focus_uses_the_scope_initial_target_before_cancel_in_declared_order() {
    let cancel = target("cancel").with_activation(ActionId::new("close").unwrap());
    let primary = target("primary").with_activation(ActionId::new("submit").unwrap());
    let initial = target("initial-field");
    let transient = target("transient");
    let mut screen = surface(&[]);
    screen.focus = FocusModel::new(vec![
        cancel.clone(),
        primary.clone(),
        initial.clone(),
        transient.clone(),
    ])
    .unwrap();
    assert!(screen.focus.focus(&initial.id));
    let mut stack = SurfaceStack::new(screen);
    assert!(stack.top_mut().focus.focus(&transient.id));
    stack
        .top_mut()
        .focus
        .replace_targets(vec![cancel, primary, initial.clone()])
        .unwrap();
    assert_eq!(
        stack.top().focus.current(),
        Some(&initial.id),
        "mount-time initial focus wins over Cancel and Submit"
    );
}

#[test]
fn absent_initial_focus_never_automatically_selects_an_action() {
    let initial = target("initial-field");
    let cancel = target("cancel").with_activation(ActionId::new("close").unwrap());
    let primary = target("primary").with_activation(ActionId::new("submit").unwrap());
    let safe = target("body");
    let mut focus =
        FocusModel::new(vec![initial, cancel.clone(), primary.clone(), safe.clone()]).unwrap();
    focus
        .replace_targets(vec![cancel.clone(), primary.clone(), safe.clone()])
        .unwrap();
    assert_eq!(
        focus.current(),
        Some(&safe.id),
        "secondary fallback uses a non-activating target"
    );
    focus.replace_targets(vec![cancel, primary]).unwrap();
    assert_eq!(
        focus.current(),
        None,
        "no safe target leaves focus empty rather than arming Cancel"
    );
}

#[test]
fn initial_disabled_activation_remains_focused_on_mount_and_after_displacement() {
    let disabled = target("initial-submit")
        .with_activation(ActionId::new("submit").unwrap())
        .with_availability(AvailabilityFn::new(|| Availability::Unavailable {
            reason: "Waiting".into(),
        }));
    let transient = target("transient");
    let mut screen = surface(&[]);
    screen.focus = FocusModel::new(vec![disabled.clone(), transient.clone()]).unwrap();
    let mut stack = SurfaceStack::new(screen);
    assert_eq!(
        stack.top().focus.current(),
        Some(&disabled.id),
        "painted disabled initial target is retained at mount"
    );
    assert!(stack.top_mut().focus.focus(&transient.id));
    stack
        .top_mut()
        .focus
        .replace_targets(vec![disabled.clone()])
        .unwrap();
    assert_eq!(
        stack.top().focus.current(),
        Some(&disabled.id),
        "only the initial activation may receive automatic fallback"
    );
    assert!(!stack.top().focus.focused().unwrap().is_enabled());
}

#[test]
fn identities_reject_absence_without_collapsing_distinct_domains() {
    assert!(TargetId::new(" ").is_err());
    assert!(ActionId::new("").is_err());
    assert!(ScopeId::new("\n").is_err());
    assert!(GroupId::new("").is_err());
    assert_ne!(
        TargetId::new("endpoint-a/session").unwrap(),
        TargetId::new("endpoint-b/session").unwrap()
    );
}

#[test]
fn focus_traversal_wraps_over_every_enabled_target_and_only_within_its_group() {
    for length in 0..=5 {
        for enabled_mask in 0..(1usize << length) {
            let targets = (0..length)
                .map(|index| {
                    let enabled = enabled_mask & (1 << index) != 0;
                    target(&format!("row-{index}"))
                        .in_group(GroupId::new(format!("group-{}", index % 2)).unwrap())
                        .with_availability(AvailabilityFn::new(move || {
                            if enabled {
                                Availability::Available
                            } else {
                                Availability::Unavailable {
                                    reason: "disabled".into(),
                                }
                            }
                        }))
                })
                .collect::<Vec<_>>();
            let enabled = targets
                .iter()
                .filter(|target| target.is_enabled())
                .map(|target| target.id.clone())
                .collect::<Vec<_>>();
            let mut focus = FocusModel::new(targets).unwrap();
            assert_eq!(
                focus.current(),
                (length > 0)
                    .then(|| TargetId::new("row-0").unwrap())
                    .as_ref(),
                "the painted initial target retains focus even when disabled"
            );
            if enabled.is_empty() {
                focus.traverse(FocusDirection::Next);
                assert!(focus.current().is_none());
                continue;
            }
            assert!(focus.focus(&enabled[0]));
            for expected in enabled.iter().cycle().skip(1).take(enabled.len() * 2) {
                focus.traverse(FocusDirection::Next);
                assert_eq!(focus.current(), Some(expected));
            }
            for expected in enabled.iter().rev().cycle().take(enabled.len() * 2) {
                focus.traverse(FocusDirection::Previous);
                assert_eq!(focus.current(), Some(expected));
            }
            for starting in &enabled {
                focus.focus(starting);
                let group = focus.focused().unwrap().group.clone();
                let grouped = focus
                    .targets()
                    .iter()
                    .filter(|target| target.is_enabled() && target.group == group)
                    .map(|target| target.id.clone())
                    .collect::<Vec<_>>();
                let position = grouped.iter().position(|id| id == starting).unwrap();
                for expected in grouped
                    .iter()
                    .cycle()
                    .skip(position + 1)
                    .take(grouped.len() * 2)
                {
                    focus.traverse_group(FocusDirection::Next);
                    assert_eq!(focus.current(), Some(expected));
                }
                focus.traverse_group(FocusDirection::Previous);
                focus.traverse_group(FocusDirection::Next);
                assert_eq!(focus.current(), Some(starting));
            }
        }
    }
    assert!(FocusModel::new(vec![target("same"), target("same")]).is_err());
}
