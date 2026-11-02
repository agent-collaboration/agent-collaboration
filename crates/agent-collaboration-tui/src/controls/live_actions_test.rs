use crate::test_support::{
    event_driver::{EventDriver, InputMode},
    live_action_scene::{LiveActionScene, LiveFixtureHandle, ProjectionKind, action, directory},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::ActionRegistry,
    surfaces::{focus_model::FocusModel, surface_stack::SurfaceId},
};
use iocraft::prelude::*;
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[tokio::test]
async fn live_title_changes_on_the_next_frame_in_every_projection() {
    for projection in [
        ProjectionKind::Button,
        ProjectionKind::Toolbar,
        ProjectionKind::Menu,
        ProjectionKind::Help,
        ProjectionKind::Hints,
    ] {
        let handle = LiveFixtureHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(LiveActionScene(journal,projection,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let rendered = |driver: &EventDriver| {
                let grid = driver.frames.last().unwrap();
                if matches!(projection, ProjectionKind::Menu | ProjectionKind::Help) {
                    grid.round_frame_text()
                } else {
                    grid.row_text(1)
                }
            };
            assert!(rendered(&driver).contains("Filters: Local"));
            let fixture = handle.0.lock().unwrap().clone().unwrap();
            let mut filter = fixture.ui.filter;
            filter.set("Remote");
            let mut revision = fixture.ui.revision;
            revision.set(1);
            // The independent revision always changes the real canvas, even on a wrong
            // build that cached the title. A missing label must be an assertion failure.
            driver.initial(&mut frames).await;
            let after = rendered(&driver);
            assert!(after.contains("Filters: Remote"), "{after}");
            assert!(!after.contains("Filters: Local"), "{after}");
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn open_model_update_switches_primary_and_keeps_identity_focus_scroll_and_draft() {
    let handle = LiveFixtureHandle::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui =
        element!(LiveActionScene(journal,projection:ProjectionKind::Dialog,handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let fixture = handle.0.lock().unwrap().clone().unwrap();
        let overlay = fixture.overlay.unwrap();
        let surface = SurfaceId::Overlay(overlay);
        assert!(fixture.stack.focus(surface, &directory()));
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![
                    (key(KeyCode::Left), true),
                    (TerminalEvent::BracketedPaste("X".into()), true),
                ],
                true,
            )
            .await;
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(
                    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                        MouseEventKind::ScrollDown,
                        20,
                        10,
                    )),
                    true,
                )],
                true,
            )
            .await;
        let before_draft = fixture.ui.draft.read().clone();
        let before_scroll = fixture.scroll.position();
        assert!(before_scroll.offset > 0);
        assert!(fixture.stack.update_overlay_if_open(overlay, |model| {
            model.focus = FocusModel::new(model.focus.targets().to_vec()).unwrap();
            model.primary_action = Some(action("login"));
        }));
        let updated = fixture.stack.snapshot();
        assert_eq!(updated.top_id(), surface);
        assert_eq!(updated.overlays().len(), 1);
        assert_eq!(updated.top().focus.current(), Some(&directory()));
        assert_eq!(updated.top().primary_action, Some(action("login")));
        assert_eq!(*fixture.ui.draft.read(), before_draft);
        assert_eq!(fixture.scroll.position(), before_scroll);
        assert!(fixture.stack.hit_regions().published().is_none());
        let mut revision = fixture.ui.revision;
        revision.set(1);
        driver.initial(&mut frames).await;
        let panel = driver.frames.last().unwrap().round_frame_text();
        assert!(panel.contains("● Log in again"), "{panel}");
        assert!(!panel.contains("● Close"));
        assert_eq!(fixture.scroll.position(), before_scroll);
        assert_eq!(*fixture.ui.draft.read(), before_draft);
        let mut enter = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
        enter.modifiers = KeyModifiers::CONTROL;
        driver
            .drive_checked(
                &mut frames,
                InputMode::Burst,
                vec![(TerminalEvent::Key(enter), true)],
                true,
                |ack| assert_eq!(ack.snapshot.0[0], (surface, b"1|directory".to_vec())),
            )
            .await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn open_model_update_survives_help_and_preserves_the_active_help_model() {
    let handle = LiveFixtureHandle::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui =
        element!(LiveActionScene(journal,projection:ProjectionKind::Dialog,handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let fixture = handle.0.lock().unwrap().clone().unwrap();
        let overlay = fixture.overlay.unwrap();
        let surface = SurfaceId::Overlay(overlay);
        assert!(fixture.stack.focus(surface, &directory()));
        let draft = fixture.ui.draft.read().clone();
        let scroll = fixture.scroll.position();
        assert!(fixture.help.show(Default::default()).unwrap());
        driver.initial(&mut frames).await;
        let help_primary = fixture.stack.snapshot().top().primary_action.clone();
        let help_focus = fixture.stack.snapshot().top().focus.current().cloned();

        assert!(fixture.stack.update_overlay_if_open(overlay, |model| {
            model.primary_action = Some(action("login"));
            let mut definitions = model.actions.definitions().to_vec();
            definitions
                .iter_mut()
                .find(|definition| definition.id == action("login"))
                .unwrap()
                .title = "Log in after refresh".into();
            model.actions =
                ActionRegistry::new(model.actions.scope().clone(), definitions).unwrap();
        }));
        let active_help_primary = fixture.stack.snapshot().top().primary_action.clone();
        assert_eq!(active_help_primary, help_primary);
        assert_eq!(
            fixture.stack.snapshot().top().focus.current(),
            help_focus.as_ref()
        );
        let mut filter = fixture.ui.filter;
        filter.set("Remote");
        let mut revision = fixture.ui.revision;
        revision.set(1);
        driver.initial(&mut frames).await;
        let help_text = driver.frames.last().unwrap().round_frame_text();
        assert!(help_text.contains("Log in after refresh"), "{help_text}");
        assert!(help_text.contains("Filters: Remote"), "{help_text}");

        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::F(9)), false)],
                false,
            )
            .await;
        assert_eq!(
            fixture.ui.logins.get(),
            0,
            "Help must not route a base action"
        );
        assert_eq!(
            fixture.stack.snapshot().top().focus.current(),
            help_focus.as_ref()
        );
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::Esc), true)],
                true,
            )
            .await;

        let restored = fixture.stack.snapshot();
        assert_eq!(restored.top_id(), surface);
        assert_eq!(restored.top().primary_action, Some(action("login")));
        assert_eq!(restored.top().focus.current(), Some(&directory()));
        assert_eq!(*fixture.ui.draft.read(), draft);
        assert_eq!(fixture.scroll.position(), scroll);
        let mut enter = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
        enter.modifiers = KeyModifiers::CONTROL;
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(TerminalEvent::Key(enter), true)],
                true,
            )
            .await;
        assert_eq!(fixture.ui.logins.get(), 1);
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn closed_model_update_does_not_call_the_updater_or_invalidate_the_current_map() {
    let handle = LiveFixtureHandle::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui =
        element!(LiveActionScene(journal,projection:ProjectionKind::Dialog,handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let fixture = handle.0.lock().unwrap().clone().unwrap();
        let overlay = fixture.overlay.unwrap();
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::Esc), true)],
                true,
            )
            .await;
        let before = fixture.ui.draft.read().clone();
        let map = fixture.stack.hit_regions().published();
        let mut calls = 0;
        assert!(!fixture.stack.update_overlay_if_open(overlay, |_| {
            calls += 1;
            let mut draft = fixture.ui.draft;
            draft.set(Default::default());
        }));
        assert_eq!(calls, 0);
        assert_eq!(*fixture.ui.draft.read(), before);
        assert_eq!(fixture.stack.hit_regions().published(), map);
        assert_eq!(fixture.stack.snapshot().top_id(), SurfaceId::Screen);
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
