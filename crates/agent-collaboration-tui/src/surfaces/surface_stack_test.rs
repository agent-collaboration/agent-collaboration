use crate::test_support::core_models::{surface, target};
use agent_collaboration_tui::{
    actions::{Availability, AvailabilityFn},
    surfaces::{
        focus_model::TargetId,
        surface_stack::{OverlayKind, OverlaySpec, SurfaceId, SurfaceStack},
    },
};
#[test]
fn closing_restores_the_captured_identity_through_reordering_and_falls_back_if_gone() {
    for replacement in [vec!["beta", "alpha"], vec!["beta"], vec![]] {
        let mut stack = SurfaceStack::new(surface(&["alpha", "beta"]));
        let first_id = stack
            .open_overlay(OverlaySpec {
                view: Default::default(),
                layout: Default::default(),
                kind: OverlayKind::Menu,
                model: surface(&["menu-item"]),
            })
            .unwrap();
        assert!(stack.is_covered(SurfaceId::Screen));
        stack
            .surface_mut(SurfaceId::Screen)
            .unwrap()
            .focus
            .replace_targets(replacement.iter().map(|name| target(name)).collect())
            .unwrap();
        stack.close_top();
        let expected = if replacement.contains(&"alpha") {
            Some("alpha")
        } else {
            replacement.first().copied()
        };
        assert_eq!(stack.top().focus.current().map(TargetId::as_str), expected);
        assert_eq!(stack.top_id(), SurfaceId::Screen);
        assert!(!stack.is_covered(SurfaceId::Screen));
        let second_id = stack
            .open_overlay(OverlaySpec {
                view: Default::default(),
                layout: Default::default(),
                kind: OverlayKind::Dialog,
                model: surface(&["close"]),
            })
            .unwrap();
        assert_ne!(first_id, second_id);
    }
    let mut stack = SurfaceStack::new(surface(&["alpha", "beta"]));
    stack
        .open_overlay(OverlaySpec {
            view: Default::default(),
            layout: Default::default(),
            kind: OverlayKind::Menu,
            model: surface(&["item"]),
        })
        .unwrap();
    let disabled =
        target("alpha").with_availability(AvailabilityFn::new(|| Availability::Unavailable {
            reason: "gone".into(),
        }));
    stack
        .surface_mut(SurfaceId::Screen)
        .unwrap()
        .focus
        .replace_targets(vec![disabled, target("beta")])
        .unwrap();
    stack.close_top();
    assert_eq!(
        stack.top().focus.current().unwrap().as_str(),
        "alpha",
        "painted disabled captured focus is retained on close"
    );
}
