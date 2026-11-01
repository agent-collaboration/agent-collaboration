use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    frame_grid::FrameGrid,
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    DialogAxisSize, DialogMargins, DialogPresentation,
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    controls::{
        fields::{Button, button_target},
        list::ScrollHandle,
    },
    dialog::{
        DialogFrame, DialogSection, DialogSections,
        help::{HelpContent, HelpControl, HelpDialog},
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
fn body() -> TargetId {
    TargetId::new("content-context-body").unwrap()
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[derive(Clone)]
struct ContentFixture {
    stack: SurfaceStackHandle,
    scroll: ScrollHandle,
}
#[derive(Props)]
struct ContentPanelProps {
    #[iocraft(required)]
    scroll: ScrollHandle,
    #[iocraft(required)]
    help: HelpControl,
    long_context: bool,
    status: u16,
}
#[component]
fn ContentPanel(props: &ContentPanelProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    if props.help.is_active(context.surface) {
        return element!(DialogFrame(title:"Help".to_owned(),scroll:props.help.scroll(context.surface).unwrap(),scroll_target:props.help.scroll_target()) {HelpDialog(control:props.help.clone())}).into_any();
    }
    let identity = agent_collaboration_tui::dialog::PinnedContext::new(if props.long_context {
        "Tokyo OpenAI Research Account Alpha Long Name"
    } else {
        "alpha"
    })
    .with_tag("alpha-tag");
    element!(DialogFrame(title:"Weekly floor".to_owned(),scroll:props.scroll.clone(),scroll_target:body(),context:Some(identity),help_action:Some(element!(Button(id:TargetId::for_action(&action("help")),action:action("help"))).into_any())) {
            DialogSections {
                DialogSection {Text(content:"Current: disabled (0%)")}
                DialogSection {Text(content:"Floor: Left 0% Right")}
                DialogSection {Text(content:"0% disables protection · maximum 15%")}
                #(match props.status {
                    1=>Some(element!(DialogSection {Text(content:"Saving preference…")}).into_any()),
                    2=>Some(element!(DialogSection {Text(content:"Save failed: retry")}).into_any()),
                    3=>Some(element!(DialogSection {Text(content:"Saved; refresh failed") Text(content:"Return to retry")}).into_any()),
                    _=>None,
                })
            }
        }).into_any()
}
#[derive(Props)]
struct ContentSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    width: u16,
    height: u16,
    long_context: bool,
    status: u16,
    handle: Arc<Mutex<Option<ContentFixture>>>,
}
#[component]
fn ContentScene(props: &ContentSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("content-context-screen").unwrap(), Vec::new())
            .unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let scroll = ScrollHandle::new(stack);
        *props.handle.lock().unwrap() = Some(ContentFixture {
            stack,
            scroll: scroll.clone(),
        });
        let help = HelpControl::new(stack, action("help"));
        let open = help.open_handler(
            HelpContent::default,
            Handler::from(|error| panic!("{error}")),
        );
        let definitions = [
            ("save", "Save", "Ctrl+Enter"),
            ("cancel", "Cancel", "Esc"),
            ("help", "Help", "F1"),
        ]
        .into_iter()
        .map(|(name, title, key)| ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            id: action(name),
            title: title.into(),
            toolbar_label: None,
            keys: vec![key.parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: if name == "help" {
                Placement::Hidden
            } else {
                Placement::Toolbar
            },
            run: if name == "help" {
                open.clone()
            } else {
                Handler::default()
            },
        })
        .collect::<Vec<_>>();
        let mut targets = vec![TargetSpec::new(body()).with_input(scroll.input_handler())];
        targets.extend(
            definitions
                .iter()
                .map(|definition| button_target(TargetId::for_action(&definition.id), definition)),
        );
        let long_context = props.long_context;
        let status = props.status;
        stack.open_overlay(OverlaySpec {
                kind:OverlayKind::Dialog,
                model:SurfaceModel {focus:FocusModel::new(targets).unwrap(),actions:ActionRegistry::new(ScopeId::new("content-context-floor").unwrap(),definitions).unwrap(),primary_action:Some(action("save"))},
                view:OverlayView::new(move ||element!(ContentPanel(scroll:scroll.clone(),help:help.clone(),long_context,status)).into_any()),
                layout:OverlayLayout::Dialog {
                    presentation:DialogPresentation::Sized {width:DialogAxisSize::Content {maximum:Some(68)},height:DialogAxisSize::Content {maximum:Some(22)}},
                    margins:DialogMargins {top:1,right:2,bottom:1,left:2},
                },
            }).unwrap();
        opened.set(true);
    }
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),Vec::new())])) ) {OverlayHost})
}
fn assert_complete(frame: &FrameGrid, fixture: &ContentFixture, inline: bool) {
    let panel = frame.round_frame_text();
    let position = fixture.scroll.position();
    assert_eq!(
        position.maximum(),
        0,
        "all content fits before the bound: {position:?}\n{panel}"
    );
    let plain = panel.replace('│', " ");
    let normalized = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    for fact in [
        "Current:",
        "disabled (0%)",
        "Floor:",
        "maximum 15%",
        "Save",
        "Cancel",
        "[alpha-tag]",
    ] {
        assert!(
            normalized.contains(fact),
            "{fact} must be visible without scrolling\n{panel}"
        );
    }
    let rows = panel.lines().collect::<Vec<_>>();
    let title = rows
        .iter()
        .position(|row| row.contains("Weekly floor"))
        .unwrap();
    let tag = rows
        .iter()
        .position(|row| row.contains("[alpha-tag]"))
        .unwrap();
    assert_eq!(
        tag,
        title + usize::from(!inline),
        "second row is used only when the declared context cannot fit inline\n{panel}"
    );
    assert!(rows.len() <= 22, "content height respects its cap\n{panel}");
}
fn assert_same_grid(actual: &FrameGrid, expected: &FrameGrid) {
    let differences = actual
        .cells
        .iter()
        .zip(&expected.cells)
        .enumerate()
        .flat_map(|(row, (actual, expected))| {
            actual.iter().zip(expected).enumerate().filter_map(
                move |(column, (actual, expected))| {
                    (actual != expected).then_some((column, row, actual, expected))
                },
            )
        })
        .take(8)
        .collect::<Vec<_>>();
    assert!(
        actual == expected,
        "exact grid differs at {differences:?}\nactual\n{}\nexpected\n{}",
        actual.round_frame_text(),
        expected.round_frame_text()
    );
}
#[tokio::test]
async fn content_dialog_counts_pinned_rows_before_squeezing_floor_guidance_and_status() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        // This fixture promises every guidance/status row is visible without scrolling.
        for (width, height, long_context) in crate::test_support::grids::STANDARD_GRIDS
            .into_iter()
            .filter(|(_, height)| *height >= 24)
            .flat_map(|(width, height)| {
                [false, true]
                    .into_iter()
                    .filter(move |long_context| !long_context || width == 120)
                    .map(move |long_context| (width, height, long_context))
            })
        {
            for status in 0u16..4 {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(ContentScene(journal,width,height,long_context,status,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    assert_complete(
                        driver.frames.last().unwrap(),
                        &fixture,
                        width >= 40 && !long_context,
                    );
                    let original = driver.frames.last().unwrap().clone();
                    if status > 0 {
                        let panel = original.round_frame_text();
                        let message = match status {
                            1 => "Saving preference…",
                            2 => "Save failed: retry",
                            _ => "Return to retry",
                        };
                        assert!(panel.contains(message), "{panel}");
                    }
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                        .await;
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                        .await;
                    assert_same_grid(driver.frames.last().unwrap(), &original);
                    assert_complete(
                        driver.frames.last().unwrap(),
                        &fixture,
                        width >= 40 && !long_context,
                    );
                })
                .await
                .unwrap();
            }
        }
    }
}
#[tokio::test]
async fn content_context_resize_keeps_guidance_and_returns_to_the_exact_initial_grid() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ContentScene(journal,width:120u16,height:36u16,status:3u16,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.lock().unwrap().clone().unwrap();
            assert_complete(
                driver.frames.last().unwrap(),
                &fixture,
                fixture.stack.viewport().0 >= 40,
            );
            let original = driver.frames.last().unwrap().clone();
            for (width, height) in crate::test_support::grids::STANDARD_GRIDS
                .into_iter()
                .filter(|(_, height)| *height >= 24)
                .cycle()
                .skip(1)
                .take(3)
            {
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Resize(width, height), true)],
                        true,
                    )
                    .await;
                assert_complete(
                    driver.frames.last().unwrap(),
                    &fixture,
                    fixture.stack.viewport().0 >= 40,
                );
            }
            assert_same_grid(driver.frames.last().unwrap(), &original);
        })
        .await
        .unwrap();
    }
}
