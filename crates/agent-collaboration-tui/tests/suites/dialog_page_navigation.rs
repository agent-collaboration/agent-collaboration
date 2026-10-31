#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{DialogFrame, HelpControl, HelpDialog, ScrollHandle, ScrollReveal},
        focus_model::{FocusModel, GroupId, TargetId, TargetSpec},
        hit_regions::{HitRegion, HitRegionKind},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    fn target(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[derive(Clone)]
    struct PageFixture {
        stack: SurfaceStackHandle,
        scroll: ScrollHandle,
        help: HelpControl,
    }
    #[derive(Props)]
    struct PageSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        handle: Arc<Mutex<Option<PageFixture>>>,
    }
    #[component]
    fn PageBody(hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let context = *hooks.use_context::<SurfaceContext>();
        element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            ScrollReveal(focused:context.is_focused(&target("source"))) {View(height:1,flex_shrink:0.0_f32) {HitRegion(target:target("source"),kind:HitRegionKind::Target) {Text(content:"Source field")}}}
            #((0..18).map(|index|element!(Text(content:format!("Form detail {index}")))))
            ScrollReveal(focused:context.is_focused(&target("destination"))) {View(height:1,flex_shrink:0.0_f32) {HitRegion(target:target("destination"),kind:HitRegionKind::Target) {Text(content:"Destination field")}}}
        })
    }
    #[derive(Props)]
    struct PageFrameProps {
        #[iocraft(required)]
        scroll: ScrollHandle,
        #[iocraft(required)]
        help: HelpControl,
    }
    #[component]
    fn PageFrame(props: &PageFrameProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let context = *hooks.use_context::<SurfaceContext>();
        if let Some(scroll) = props.help.scroll(context.surface) {
            return element!(DialogFrame(title:"Form pages".to_owned(),padding:Some(0u16),scroll,scroll_target:props.help.scroll_target(),scroll_focus:Some(props.help.body_target())) {HelpDialog(control:props.help.clone())}).into_any();
        }
        element!(DialogFrame(title:"Form pages".to_owned(),padding:Some(0u16),scroll:props.scroll.clone(),scroll_target:target("body")) {PageBody}).into_any()
    }
    #[component]
    fn PageScene(props: &PageSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
            focus: FocusModel::new(Vec::new()).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("pages-screen").unwrap(), Vec::new())
                .unwrap(),
            primary_action: None,
        });
        let mut opened = hooks.use_ref(|| false);
        if !opened.get() {
            let scroll = ScrollHandle::new(stack);
            let help = HelpControl::new(stack, ActionId::new("help").unwrap());
            let view_help = help.clone();
            let help_action = ActionDef {
                id: ActionId::new("help").unwrap(),
                title: "Help".into(),
                toolbar_label: None,
                keys: vec!["F1".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: help.open_handler(Default::default, Handler::from(|error| panic!("{error}"))),
            };
            let input = scroll.pages_input_handler();
            let group = GroupId::new("form-fields").unwrap();
            let view_scroll = scroll.clone();
            stack
                .open_overlay(OverlaySpec {
                    kind: OverlayKind::Dialog,
                    model: SurfaceModel {
                        focus: FocusModel::new(vec![
                            TargetSpec::new(target("source"))
                                .in_group(group.clone())
                                .with_input(input.clone()),
                            TargetSpec::new(target("destination"))
                                .in_group(group)
                                .with_input(input.clone()),
                            TargetSpec::new(target("body"))
                                .pointer_only()
                                .with_input(input),
                        ])
                        .unwrap(),
                        actions: ActionRegistry::new(
                            ScopeId::new("pages-form").unwrap(),
                            vec![help_action],
                        )
                        .unwrap(),
                        primary_action: None,
                    },
                    view: OverlayView::new(move || {
                        element!(PageFrame(scroll:view_scroll.clone(),help:view_help.clone()))
                            .into_any()
                    }),
                    layout: OverlayLayout::CenteredCells {
                        width: 42,
                        height: 11,
                    },
                })
                .unwrap();
            *props.handle.lock().unwrap() = Some(PageFixture {
                stack,
                scroll,
                help,
            });
            opened.set(true);
        }
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:76u16,height:26u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),stack.snapshot().top().focus.current().unwrap().as_str().as_bytes().to_vec())]))) {OverlayHost})
    }
    #[tokio::test]
    async fn form_pages_and_wheel_scroll_while_arrows_move_field_focus_and_home_undoes_focus_reveal()
     {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(PageScene(journal,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                let initial = fixture.scroll.position();
                assert_eq!(initial.offset, 0);
                assert!(
                    initial.maximum() > initial.viewport_height,
                    "long form can page"
                );
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Down), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0].1, b"destination",
                                "Down moves field focus instead of scrolling"
                            )
                        },
                    )
                    .await;
                assert_eq!(
                    fixture.scroll.position().offset,
                    initial.maximum(),
                    "focus reveals the last field"
                );
                for (code, expected) in [
                    (KeyCode::Home, 0),
                    (KeyCode::End, initial.maximum()),
                    (
                        KeyCode::PageUp,
                        initial.maximum().saturating_sub(initial.viewport_height),
                    ),
                    (KeyCode::PageDown, initial.maximum()),
                    (KeyCode::Home, 0),
                ] {
                    driver
                        .drive_checked(&mut frames, mode, vec![(key(code), true)], true, |ack| {
                            assert_eq!(
                                ack.snapshot.0[0].1, b"destination",
                                "paging keeps field focus"
                            );
                            assert_eq!(
                                fixture.scroll.position().offset,
                                expected,
                                "pages-only input handles {code:?} before draw"
                            );
                        })
                        .await;
                    assert_eq!(
                        fixture.scroll.position().offset,
                        expected,
                        "focus must not immediately undo {code:?} scrolling"
                    );
                }
                let map = fixture.stack.hit_regions().published().unwrap();
                let point = *map
                    .targets
                    .iter()
                    .find(|region| {
                        region.target == target("body") && region.kind == HitRegionKind::ScrollArea
                    })
                    .unwrap()
                    .cells
                    .first()
                    .unwrap();
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::ScrollDown,
                                point.0,
                                point.1,
                            )),
                            true,
                        )],
                        true,
                        |ack| {
                            assert_eq!(ack.snapshot.0[0].1, b"destination");
                            assert_eq!(
                                fixture.scroll.position().offset,
                                1,
                                "wheel uses the same body scroll handle"
                            );
                        },
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Up), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0].1, b"source",
                                "Up moves field focus instead of scrolling"
                            )
                        },
                    )
                    .await;
                assert_eq!(
                    fixture.scroll.position().offset,
                    0,
                    "focus reveals Source again"
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn option_pages_and_bounds_scroll_form_bodies_without_editing_or_moving_field_focus() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(PageScene(journal,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                let initial = fixture.scroll.position();
                for (code, modifiers, expected) in [
                    (KeyCode::Down, KeyModifiers::ALT, initial.viewport_height),
                    (
                        KeyCode::Down,
                        KeyModifiers::ALT | KeyModifiers::SHIFT,
                        initial.maximum(),
                    ),
                    (
                        KeyCode::Up,
                        KeyModifiers::ALT,
                        initial.maximum().saturating_sub(initial.viewport_height),
                    ),
                    (KeyCode::Up, KeyModifiers::ALT | KeyModifiers::SHIFT, 0),
                ] {
                    let mut event = KeyEvent::new(KeyEventKind::Press, code);
                    event.modifiers = modifiers;
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::Key(event), true)],
                            true,
                            |ack| {
                                assert_eq!(
                                    ack.snapshot.0[0].1, b"source",
                                    "paging aliases keep focused field"
                                );
                                assert_eq!(
                                    fixture.scroll.position().offset,
                                    expected,
                                    "Option page and bound aliases reach the shared scroll model"
                                );
                            },
                        )
                        .await;
                }
                for code in [KeyCode::Up, KeyCode::Down] {
                    let mut event = KeyEvent::new(KeyEventKind::Press, code);
                    event.modifiers = KeyModifiers::CONTROL;
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::Key(event), false)],
                            false,
                            |ack| {
                                assert_eq!(ack.snapshot.0[0].1, b"source");
                                assert_eq!(fixture.scroll.position().offset, 0);
                            },
                        )
                        .await;
                }
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Down), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0].1, b"destination",
                                "plain Down still leaves the field"
                            )
                        },
                    )
                    .await;
            })
            .await
            .unwrap();
        }
    }
    #[tokio::test]
    async fn closing_help_keeps_the_form_manual_scroll_instead_of_revealing_its_unchanged_focused_field_again()
     {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(PageScene(journal,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                let parent = fixture.stack.snapshot().top_id();
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Down), true)], true)
                    .await;
                assert_eq!(
                    fixture.scroll.position().offset,
                    fixture.scroll.position().maximum(),
                    "focus reveal has drawn before manual scrolling"
                );
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Home), true)], true)
                    .await;
                assert_eq!(
                    fixture.stack.snapshot().top().focus.current(),
                    Some(&target("destination"))
                );
                let position = fixture.scroll.position();
                assert_eq!(position.offset, 0);
                let before = driver.frames.last().unwrap().clone();
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::F(1)), true)],
                        true,
                        |_| assert!(fixture.help.is_active(parent)),
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Esc), true)],
                        true,
                        |_| assert!(!fixture.help.is_active(parent)),
                    )
                    .await;
                assert_eq!(
                    fixture.scroll.position().natural_height,
                    position.natural_height,
                    "same form content"
                );
                assert_eq!(
                    fixture.scroll.position().viewport_height,
                    position.viewport_height,
                    "same form viewport"
                );
                assert_eq!(
                    fixture.scroll.position().offset,
                    position.offset,
                    "Help close restores manual scroll instead of repeating old focus reveal"
                );
                assert_eq!(
                    driver.frames.last().unwrap(),
                    &before,
                    "same manual form viewport after Help"
                );
            })
            .await
            .unwrap();
        }
    }
}
