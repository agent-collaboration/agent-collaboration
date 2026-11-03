use crate::{
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionRisk, AvailabilityFn, Placement, ScopeId,
    },
    controls::{
        fields::{Button, TextField, TextFieldState, button_target},
        list::{ScrollHandle, ScrollList},
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    },
    test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
type Observed = Arc<Mutex<Option<(ScrollHandle, SurfaceStackHandle)>>>;
fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}
#[derive(Props)]
struct RevealSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    #[iocraft(required)]
    observed: Observed,
    width: u16,
    height: u16,
    partial: bool,
    oversized: bool,
}
#[component]
fn RevealScene(props: &RevealSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let value = hooks.use_state(|| TextFieldState::new("Editor"));
    let mut retained = hooks.use_ref(|| None);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let scroll = ScrollHandle::new(stack);
        retained.set(Some(scroll));
        let button_height = props.height + 5;
        let action = ActionDef {
            risk: ActionRisk::Ordinary,
            id: ActionId::new("run").unwrap(),
            title: if props.oversized {
                crate::actions::ActionTitle::dynamic(move || {
                    (0..button_height)
                        .map(|index| format!("Button line {index}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            } else {
                "Run\nSecond\nThird".into()
            },
            toolbar_label: None,
            keys: Vec::new(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run: Handler::default(),
        };
        SurfaceModel {
            focus: FocusModel::new(vec![
                TargetSpec::new(target("first")),
                button_target(target("button"), &action),
                TargetSpec::new(target("last")),
            ])
            .unwrap(),
            actions: ActionRegistry::new(ScopeId::new("reveal").unwrap(), vec![action]).unwrap(),
            primary_action: None,
        }
    });
    let scroll = retained.read().as_ref().unwrap().clone();
    *props.observed.lock().unwrap() = Some((scroll.clone(), stack));
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,Vec::new())]))) {OverlayHost {
        ScrollList(id:target("scroll"),scroll,grow:true) {
            TextField(id:target("first"),value)
            View(height:if props.partial {props.height-2}else{props.height+4},flex_shrink:0.0_f32)
            Button(id:target("button"),action:ActionId::new("run").unwrap())
            View(height:props.height+4,flex_shrink:0.0_f32)
            TextField(id:target("last"),value)
        }
    }})
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[tokio::test]
async fn keyboard_focus_reveals_buttons_and_fields_and_aligns_oversized_targets_at_all_grids() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        for oversized in [false, true] {
            let observed = Observed::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui =
                element!(RevealScene(journal,observed:observed.clone(),width,height,oversized));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(KeyCode::Tab), true)],
                        true,
                    )
                    .await;
                let grid = driver.frames.last().unwrap();
                let text = (0..grid.height)
                    .map(|row| grid.row_text(row))
                    .collect::<Vec<_>>()
                    .join("\n");
                if oversized {
                    assert!(
                        grid.row_text(0).contains("Button line 0"),
                        "oversized target starts at its top: {text}"
                    );
                } else {
                    assert!(
                        text.contains("Run") && text.contains("Second") && text.contains("Third"),
                        "whole focused button is revealed: {text}"
                    );
                }
                let (scroll, stack) = observed.lock().unwrap().clone().unwrap();
                assert_eq!(
                    stack.snapshot().top().focus.current(),
                    Some(&target("button"))
                );
                assert!(
                    scroll.position().offset > 0,
                    "keyboard focus moves the scrolling body"
                );
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(KeyCode::Tab), true)],
                        true,
                    )
                    .await;
                let published = stack.hit_regions().published().unwrap();
                assert!(
                    published
                        .targets
                        .iter()
                        .any(|region| region.target == target("last") && !region.cells.is_empty()),
                    "the next focused field is visible"
                );
                let grid = driver.frames.last().unwrap();
                assert!((0..grid.height).any(|row| grid.row_text(row).contains("Editor")));
            })
            .await
            .unwrap();
        }
    }
}

#[tokio::test]
async fn pointer_focus_measures_a_partially_visible_button_without_scrolling_it() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        let observed = Observed::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui =
            element!(RevealScene(journal,observed:observed.clone(),width,height,partial:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let (scroll, stack) = observed.lock().unwrap().clone().unwrap();
            let before = scroll.position().offset;
            let map = stack.hit_regions().published().unwrap();
            let point = map
                .targets
                .iter()
                .find(|region| region.target == target("button"))
                .unwrap()
                .cells[0];
            let click = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Down(MouseButton::Left),
                point.0,
                point.1,
            ));
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(click, true)],
                    true,
                )
                .await;
            assert_eq!(
                stack.snapshot().top().focus.current(),
                Some(&target("button"))
            );
            assert_eq!(
                scroll.position().offset,
                before,
                "pointer focus preserves viewport"
            );
        })
        .await
        .unwrap();
    }
}
