use crate::{
    surfaces::{
        focus_model::FocusModel,
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId},
    },
    test_support::{
        core_models::surface,
        event_driver::EventDriver,
        grids::STANDARD_GRIDS,
        live_action_scene::{
            LiveActionScene, LiveFixture, LiveFixtureHandle, ProjectionKind, action, directory,
        },
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;
use std::cell::{Cell, RefCell};

async fn with_dialog(check: impl Fn(LiveFixture)) {
    for (width, height) in STANDARD_GRIDS {
        let handle = LiveFixtureHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let observed = journal.clone();
        let mut ui = element!(LiveActionScene(journal,projection:ProjectionKind::Dialog,width:Some(width),height:Some(height),handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let before = observed.draw_count();
            check(handle.0.lock().unwrap().clone().unwrap());
            assert_eq!(
                observed.draw_count(),
                before,
                "reentry facts are immediate; the check never polls rendering"
            );
        })
        .await
        .unwrap();
    }
}
fn new_menu() -> OverlaySpec {
    OverlaySpec {
        kind: OverlayKind::Menu,
        model: surface(&[]),
        view: OverlayView::default(),
        layout: OverlayLayout::default(),
    }
}
#[tokio::test]
async fn closing_during_model_update_wins_without_resurrection_or_a_second_callback() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let calls = Cell::new(0);
        let result = fixture.stack.update_overlay_if_open(overlay, |candidate| {
            calls.set(calls.get() + 1);
            candidate.primary_action = None;
            assert_eq!(fixture.stack.close_top(), Some(overlay));
        });
        assert!(
            !result,
            "a close during the updater rejects the detached candidate"
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(fixture.stack.snapshot().top_id(), SurfaceId::Screen);
        assert!(
            fixture.stack.snapshot().overlays().is_empty(),
            "the closed overlay is never resurrected"
        );
        let mut closed_calls = 0;
        assert!(
            !fixture
                .stack
                .update_overlay_if_open(overlay, |_| closed_calls += 1)
        );
        assert_eq!(
            closed_calls, 0,
            "already-closed targets do not invoke an updater"
        );
    })
    .await;
}
#[tokio::test]
async fn opening_during_model_update_keeps_the_new_overlay_and_identity_sequence() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let opened = Cell::new(None);
        let calls = Cell::new(0);
        let before = fixture.stack.snapshot().top().primary_action.clone();
        let result = fixture.stack.update_overlay_if_open(overlay, |candidate| {
            calls.set(calls.get() + 1);
            candidate.primary_action = None;
            opened.set(Some(fixture.stack.open_overlay(new_menu()).unwrap()));
        });
        assert!(
            !result,
            "an open during the updater rejects its stale candidate"
        );
        assert_eq!(calls.get(), 1);
        let opened = opened.get().unwrap();
        let snapshot = fixture.stack.snapshot();
        assert_eq!(
            snapshot
                .overlays()
                .iter()
                .map(|overlay| overlay.id)
                .collect::<Vec<_>>(),
            vec![overlay, opened]
        );
        assert_eq!(snapshot.top_id(), SurfaceId::Overlay(opened));
        assert_eq!(
            snapshot.overlay_base_model(overlay).unwrap().primary_action,
            before
        );
        assert_eq!(fixture.stack.close_top(), Some(opened));
        let next = fixture.stack.open_overlay(new_menu()).unwrap();
        assert_ne!(
            next, opened,
            "an intervening open never rolls back the identity counter"
        );
    })
    .await;
}
#[tokio::test]
async fn help_opened_during_model_update_keeps_its_active_model_and_untouched_base() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let surface = SurfaceId::Overlay(overlay);
        let calls = Cell::new(0);
        let before = fixture.stack.snapshot().top().primary_action.clone();
        let help_primary = RefCell::new(None);
        let result = fixture.stack.update_overlay_if_open(overlay, |candidate| {
            calls.set(calls.get() + 1);
            candidate.primary_action = None;
            assert!(fixture.help.show(Default::default()).unwrap());
            *help_primary.borrow_mut() = fixture.stack.snapshot().top().primary_action.clone();
        });
        assert!(
            !result,
            "Help opened during the callback wins over the detached base update"
        );
        assert_eq!(calls.get(), 1);
        assert!(fixture.help.is_active(surface));
        let snapshot = fixture.stack.snapshot();
        assert_eq!(snapshot.top_id(), surface);
        assert_eq!(
            snapshot.top().primary_action.as_ref(),
            help_primary.borrow().as_ref()
        );
        assert_eq!(
            snapshot.overlay_base_model(overlay).unwrap().primary_action,
            before
        );
    })
    .await;
}
#[tokio::test]
async fn a_nested_model_update_is_retained_instead_of_overwritten_by_its_outer_candidate() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let outer_calls = Cell::new(0);
        let inner_calls = Cell::new(0);
        let result = fixture.stack.update_overlay_if_open(overlay, |candidate| {
            outer_calls.set(outer_calls.get() + 1);
            candidate.primary_action = None;
            assert!(
                fixture.stack.update_overlay_if_open(overlay, |inner| {
                    inner_calls.set(inner_calls.get() + 1);
                    inner.primary_action = Some(action("login"));
                }),
                "the inner non-reentrant update commits"
            );
        });
        assert!(
            !result,
            "the nested publication rejects the outer candidate"
        );
        assert_eq!(outer_calls.get(), 1);
        assert_eq!(inner_calls.get(), 1);
        assert_eq!(
            fixture.stack.snapshot().top().primary_action,
            Some(action("login"))
        );
    })
    .await;
}
#[tokio::test]
async fn help_open_and_close_during_the_callback_is_detected_even_when_topology_returns() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let surface = SurfaceId::Overlay(overlay);
        let calls = Cell::new(0);
        let before = fixture.stack.snapshot().top().primary_action.clone();
        let result = fixture.stack.update_overlay_if_open(overlay, |candidate| {
            calls.set(calls.get() + 1);
            candidate.primary_action = None;
            assert!(fixture.help.show(Default::default()).unwrap());
            assert!(fixture.help.is_active(surface));
            assert!(fixture.help.close());
            assert!(!fixture.help.is_active(surface));
            assert_eq!(fixture.stack.snapshot().top_id(), surface);
        });
        assert!(
            !result,
            "Help open-close ABA is still an intervening publication"
        );
        assert_eq!(calls.get(), 1);
        let snapshot = fixture.stack.snapshot();
        assert_eq!(snapshot.overlays().len(), 1);
        assert_eq!(snapshot.top_id(), surface);
        assert_eq!(snapshot.top().primary_action, before);
    })
    .await;
}
#[tokio::test]
async fn a_reentrant_no_op_publication_rejects_the_candidate_without_extra_invalidation() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let calls = Cell::new(0);
        let map_after_callback = RefCell::new(None);
        let before = fixture.stack.snapshot().top().primary_action.clone();
        let result = fixture.stack.update_overlay_if_open(overlay, |candidate| {
            calls.set(calls.get() + 1);
            candidate.primary_action = None;
            fixture.stack.update(|_| {});
            *map_after_callback.borrow_mut() = fixture.stack.hit_regions().published();
        });
        assert!(
            map_after_callback.borrow().is_some(),
            "fixture publishes real geometry before challenging rejected invalidation"
        );
        assert!(
            !result,
            "even a no-op stack publication rejects the outer candidate"
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(fixture.stack.snapshot().top().primary_action, before);
        assert_eq!(
            fixture.stack.hit_regions().published().as_ref(),
            map_after_callback.borrow().as_ref(),
            "a rejected candidate adds no hit-map invalidation"
        );
    })
    .await;
}
#[tokio::test]
async fn ordinary_and_already_help_suspended_updates_still_commit_with_identity_and_focus() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let surface = SurfaceId::Overlay(overlay);
        let calls = Cell::new(0);
        assert!(fixture.stack.focus(surface, &directory()));
        let draft = fixture.ui.draft.read().clone();
        let scroll = fixture.scroll.position();
        assert!(
            fixture.stack.hit_regions().published().is_some(),
            "fixture has a published map before a successful model update"
        );
        assert!(
            fixture.stack.update_overlay_if_open(overlay, |candidate| {
                calls.set(calls.get() + 1);
                candidate.focus = FocusModel::new(candidate.focus.targets().to_vec()).unwrap();
                candidate.primary_action = Some(action("login"));
            }),
            "a non-reentrant update still commits"
        );
        assert_eq!(calls.get(), 1);
        let snapshot = fixture.stack.snapshot();
        assert_eq!(snapshot.top_id(), surface);
        assert_eq!(snapshot.top().focus.current(), Some(&directory()));
        assert_eq!(snapshot.top().primary_action, Some(action("login")));
        assert!(fixture.stack.hit_regions().published().is_none());
        assert_eq!(*fixture.ui.draft.read(), draft);
        assert_eq!(fixture.scroll.position(), scroll);
        assert!(fixture.help.show(Default::default()).unwrap());
        let help_primary = fixture.stack.snapshot().top().primary_action.clone();
        assert!(
            fixture.stack.update_overlay_if_open(overlay, |candidate| {
                calls.set(calls.get() + 1);
                candidate.primary_action = Some(action("close"));
            }),
            "an already-suspended base is still eligible for a non-reentrant update"
        );
        assert_eq!(calls.get(), 2);
        let snapshot = fixture.stack.snapshot();
        assert!(fixture.help.is_active(surface));
        assert_eq!(snapshot.top().primary_action, help_primary);
        assert_eq!(
            snapshot.overlay_base_model(overlay).unwrap().primary_action,
            Some(action("close"))
        );
        assert_eq!(
            snapshot
                .overlay_base_model(overlay)
                .unwrap()
                .focus
                .current(),
            Some(&directory())
        );
        assert_eq!(*fixture.ui.draft.read(), draft);
        assert_eq!(fixture.scroll.position(), scroll);
    })
    .await;
}

#[tokio::test]
async fn conditional_publication_preserves_a_newer_stack_and_does_not_advance_its_token() {
    with_dialog(|fixture| {
        let overlay = fixture.overlay.unwrap();
        let stale_stack = fixture.stack.snapshot();
        let observed_token = std::sync::Arc::clone(&fixture.stack.slot.read().write_token);
        assert!(fixture.stack.update_overlay_if_open(overlay, |candidate| {
            candidate.primary_action = Some(action("login"));
        }));
        let newer_token = std::sync::Arc::clone(&fixture.stack.slot.read().write_token);
        assert!(
            !fixture
                .stack
                .publish_stack(stale_stack, Some(&observed_token)),
            "conditional publication rejects a stale observed token"
        );
        assert_eq!(
            fixture.stack.snapshot().top().primary_action,
            Some(action("login")),
            "rejected publication retains the newer model"
        );
        assert!(
            std::sync::Arc::ptr_eq(&newer_token, &fixture.stack.slot.read().write_token),
            "rejection does not itself publish a token"
        );
    })
    .await;
}
