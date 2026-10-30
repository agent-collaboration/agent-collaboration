#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionId, ActionRegistry, ScopeId},
        controls::{DialogFrame, ScrollHandle, ScrollReveal},
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
            let input = scroll.pages_input_handler();
            let group = GroupId::new("form-fields").unwrap();
            let view_scroll = scroll.clone();
            stack.open_overlay(OverlaySpec {
                kind:OverlayKind::Dialog,
                model:SurfaceModel {
                    focus:FocusModel::new(vec![TargetSpec::new(target("source")).in_group(group.clone()).with_input(input.clone()),TargetSpec::new(target("destination")).in_group(group).with_input(input.clone()),TargetSpec::new(target("body")).pointer_only().with_input(input)]).unwrap(),
                    actions:ActionRegistry::new(ScopeId::new("pages-form").unwrap(),Vec::new()).unwrap(),primary_action:None,
                },
                view:OverlayView::new(move ||element!(DialogFrame(title:"Form pages".to_owned(),padding:Some(0u16),scroll:view_scroll.clone(),scroll_target:target("body")) {PageBody}).into_any()),
                layout:OverlayLayout::CenteredCells {width:42,height:11},
            }).unwrap();
            *props.handle.lock().unwrap() = Some(PageFixture { stack, scroll });
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
}
