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
        DialogFrame,
        help::{HelpContent, HelpContextLine, HelpControl, HelpDialog},
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
    TargetId::new("pinned-body").unwrap()
}
#[derive(Clone)]
struct PinnedFixture {
    stack: SurfaceStackHandle,
    scroll: ScrollHandle,
}
#[derive(Props)]
struct PinnedPanelProps {
    custom_tag: Option<String>,
    #[iocraft(required)]
    credits: State<bool>,
    #[iocraft(required)]
    scroll: ScrollHandle,
    #[iocraft(required)]
    help: HelpControl,
}
#[component]
fn PinnedPanel(props: &PinnedPanelProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let surface = *hooks.use_context::<SurfaceContext>();
    if props.help.is_active(surface.surface) {
        return element!(DialogFrame(title:"Help".to_owned(),scroll:props.help.scroll(surface.surface).unwrap(),scroll_target:props.help.scroll_target()) {HelpDialog(control:props.help.clone())}).into_any();
    }
    let identity = agent_collaboration_tui::dialog::PinnedContext::new("東京").with_tag(
        props
            .custom_tag
            .clone()
            .unwrap_or_else(|| "alpha-tag".to_owned()),
    );
    let credits = props.credits.get();
    element!(DialogFrame(title:if credits {"Options · Credits".to_owned()}else{"Options · Resets".to_owned()},scroll:props.scroll.clone(),scroll_target:body(),context:Some(identity),help_action:Some(element!(Button(id:TargetId::for_action(&action("help")),action:action("help"))).into_any())) {
            View(column_gap:2,flex_shrink:0.0_f32) {Text(content:"Resets") Text(content:"Credits")}
            Text(content:if credits {"Credit balance"}else{"Live eligibility"})
            #((0..40).map(|row|element!(Text(content:format!("{}_{row}",if credits {"CREDIT_ROW"}else{"RESET_ROW"})))))
        }).into_any()
}
#[derive(Props)]
struct PinnedSceneProps {
    custom_tag: Option<String>,
    #[iocraft(required)]
    journal: EventJournal,
    width: u16,
    height: u16,
    handle: Arc<Mutex<Option<PinnedFixture>>>,
}
#[component]
fn PinnedScene(props: &PinnedSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let credits = hooks.use_state(|| false);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("pinned-screen").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let scroll = ScrollHandle::new(stack);
        *props.handle.lock().unwrap() = Some(PinnedFixture {
            stack,
            scroll: scroll.clone(),
        });
        let help = HelpControl::new(stack, action("help"));
        let help_run = help.open_handler(
            || HelpContent {
                context: vec![HelpContextLine {
                    label: "Captured account".into(),
                    value: "東京 [alpha-tag]".into(),
                }],
                conventions: Vec::new(),
            },
            Handler::from(|error| panic!("{error}")),
        );
        let definitions = [
            ("close", "Close", "Esc"),
            ("remove", "Remove", "Delete"),
            ("help", "Help", "F1"),
            ("credits", "Credits", "F2"),
            ("resets", "Resets", "F3"),
        ]
        .into_iter()
        .map(|(name, title, key)| {
            let change_scroll = scroll.clone();
            let run = match name {
                "help" => help_run.clone(),
                "close" => Handler::from(move |()| {
                    stack.close_top();
                }),
                "credits" | "resets" => Handler::from(move |()| {
                    let mut credits = credits;
                    credits.set(name == "credits");
                    change_scroll.reset();
                }),
                _ => Handler::from(|()| panic!("navigation must not run Remove")),
            };
            ActionDef {
                risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
                id: action(name),
                title: title.into(),
                toolbar_label: None,
                keys: vec![key.parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: if matches!(name, "close" | "remove") {
                    Placement::Toolbar
                } else {
                    Placement::Hidden
                },
                run,
            }
        })
        .collect::<Vec<_>>();
        let mut targets = vec![TargetSpec::new(body()).with_input(scroll.input_handler())];
        targets.extend(
            definitions
                .iter()
                .filter(|definition| matches!(definition.id.as_str(), "close" | "remove" | "help"))
                .map(|definition| button_target(TargetId::for_action(&definition.id), definition)),
        );
        let custom_tag = props.custom_tag.clone();
        stack
                .open_overlay(OverlaySpec {
                    kind: OverlayKind::Dialog,
                    model: SurfaceModel {
                        focus: FocusModel::new(targets).unwrap(),
                        actions: ActionRegistry::new(
                            ScopeId::new("pinned-options").unwrap(),
                            definitions,
                        )
                        .unwrap(),
                        primary_action: None,
                    },
                    view: OverlayView::new(move || {
                        element!(PinnedPanel(credits,scroll:scroll.clone(),help:help.clone(),custom_tag:custom_tag.clone()))
                            .into_any()
                    }),
                    layout: OverlayLayout::Dialog {
                        presentation: DialogPresentation::Sized {
                            width: DialogAxisSize::Content { maximum: Some(88) },
                            height: DialogAxisSize::Content { maximum: Some(24) },
                        },
                        margins: DialogMargins {
                            top: 1,
                            right: 2,
                            bottom: 1,
                            left: 2,
                        },
                    },
                })
                .unwrap();
        opened.set(true);
    }
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),vec![u8::from(credits.get())])])) ) {OverlayHost})
}
fn assert_pinned(frame: &FrameGrid, title: &str, fixture: &PinnedFixture) {
    assert_eq!(
        fixture.stack.snapshot().overlays().len(),
        1,
        "one live task scope"
    );
    let panel = frame.round_frame_text();
    let rows = panel.lines().collect::<Vec<_>>();
    let title_row = rows.iter().position(|row| row.contains(title)).unwrap();
    let tag_row = rows
        .iter()
        .position(|row| row.contains("[alpha-tag]"))
        .unwrap_or_else(|| panic!("complete captured tag remains pinned\n{panel}"));
    assert_eq!(
        tag_row,
        title_row + 1,
        "context gets its own pinned row\n{panel}"
    );
    assert!(
        rows[tag_row].contains("東京"),
        "child layout preserves the complete CJK prefix\n{panel}"
    );
    assert!(
        fixture.scroll.position().viewport_height >= 3,
        "second pinned row keeps at least three body rows\n{panel}"
    );
    assert!(
        !panel.contains("Help F1"),
        "Help yields before the captured identity\n{panel}"
    );
}
#[tokio::test]
async fn phone_options_keeps_captured_tag_on_its_own_row_in_all_five_poses() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(PinnedScene(journal,width:32u16,height:24u16,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let fixture = handle.lock().unwrap().clone().unwrap();
            assert_pinned(driver.frames.last().unwrap(), "Options · Resets", &fixture);
            assert!(
                driver
                    .frames
                    .last()
                    .unwrap()
                    .round_frame_text()
                    .contains("Live eligibility")
            );
            let before_help = driver.frames.last().unwrap().clone();
            let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
            driver
                .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                .await;
            driver
                .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                .await;
            assert_eq!(
                driver.frames.last().unwrap(),
                &before_help,
                "Help restores the exact phone task"
            );
            driver
                .drive(&mut frames, mode, vec![(key(KeyCode::End), true)], true)
                .await;
            assert_pinned(driver.frames.last().unwrap(), "Options · Resets", &fixture);
            assert!(
                driver
                    .frames
                    .last()
                    .unwrap()
                    .round_frame_text()
                    .contains("RESET_ROW_39")
            );
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![(key(KeyCode::F(2)), true)],
                    true,
                    |ack| assert_eq!(ack.snapshot.0[0].1, vec![1]),
                )
                .await;
            assert_pinned(driver.frames.last().unwrap(), "Options · Credits", &fixture);
            assert!(
                driver
                    .frames
                    .last()
                    .unwrap()
                    .round_frame_text()
                    .contains("Credit balance")
            );
            driver
                .drive(&mut frames, mode, vec![(key(KeyCode::End), true)], true)
                .await;
            assert_pinned(driver.frames.last().unwrap(), "Options · Credits", &fixture);
            assert!(
                driver
                    .frames
                    .last()
                    .unwrap()
                    .round_frame_text()
                    .contains("CREDIT_ROW_39")
            );
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![(key(KeyCode::F(3)), true)],
                    true,
                    |ack| assert_eq!(ack.snapshot.0[0].1, vec![0]),
                )
                .await;
            assert_pinned(driver.frames.last().unwrap(), "Options · Resets", &fixture);
            assert!(
                driver
                    .frames
                    .last()
                    .unwrap()
                    .round_frame_text()
                    .contains("Live eligibility")
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn second_context_row_is_used_only_when_three_body_rows_remain() {
    for (height, separate) in [(13u16, true), (12, false)] {
        let handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(PinnedScene(journal,width:32u16,height,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.lock().unwrap().clone().unwrap();
            let panel = driver.frames.last().unwrap().round_frame_text();
            assert_eq!(
                fixture.scroll.position().viewport_height,
                3,
                "body gets exactly its three reserved rows\n{panel}"
            );
            if separate {
                assert_pinned(driver.frames.last().unwrap(), "Options · Resets", &fixture);
            } else {
                let title = panel.lines().find(|row| row.contains("Options")).unwrap();
                assert!(
                    title.contains('…') && title.contains("[alpha-tag]"),
                    "text yields to the whole tag without stealing the third body row\n{panel}"
                );
            }
        })
        .await
        .unwrap();
    }
}
#[tokio::test]
async fn a_tag_that_fits_the_row_yields_no_cells_to_a_title_or_name() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(PinnedScene(journal,width:32u16,height:12u16,custom_tag:Some("abcdefghijklmnopqrstuvwx".to_owned())));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        let panel = driver.frames.last().unwrap().round_frame_text();
        assert!(
            panel.contains("[abcdefghijklmnopqrstuvwx]"),
            "the whole26celltag fits before any title or name\n{panel}"
        );
        assert!(panel.contains("Live eligibility"), "{panel}");
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn short_and_phone_options_keep_the_whole_tag_and_initial_task_fact() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS
        .into_iter()
        .filter(|(width, _)| *width <= 48)
    {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(PinnedScene(journal, width, height));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let panel = driver.frames.last().unwrap().round_frame_text();
            assert!(
                panel.contains("[alpha-tag]") && panel.contains("Live eligibility"),
                "whole identity and task fact at {width}x{height}\n{panel}"
            );
            assert!(
                panel.contains("Close") && panel.contains("Remove"),
                "{panel}"
            );
        })
        .await
        .unwrap();
    }
}
#[tokio::test]
async fn wide_options_grid_sizes_to_typed_natural_context_and_retains_body_capacity() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(PinnedScene(journal,width:120u16,height:36u16));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        let panel = driver.frames.last().unwrap().round_frame_text();
        assert_eq!(panel.lines().count(), 24);
        assert!(panel.contains("RESET_ROW_13") && !panel.contains("RESET_ROW_14"));
        assert!(panel.contains("Help F1") && panel.contains("[alpha-tag]"));
        assert_eq!(
            panel,
            include_str!("test_fixtures/options-120x36.txt").trim_end(),
            "typed context removes artificial allocation; body capacity and actions remain"
        );
    })
    .await
    .unwrap();
}
