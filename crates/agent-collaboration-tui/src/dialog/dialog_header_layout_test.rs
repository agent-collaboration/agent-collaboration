use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
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
        surface_handle::{SurfaceContext, use_surface_stack},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}
fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
#[derive(Props)]
struct HeaderPanelProps {
    #[iocraft(required)]
    help: HelpControl,
    #[iocraft(required)]
    scroll: ScrollHandle,
    title: String,
    clear_context_on_resize: bool,
}
#[component]
fn HeaderPanel(props: &HeaderPanelProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let surface = *hooks.use_context::<SurfaceContext>();
    let active = props.help.is_active(surface.surface);
    let scroll = props
        .help
        .scroll(surface.surface)
        .unwrap_or_else(|| props.scroll.clone());
    let scroll_target = if active {
        props.help.scroll_target()
    } else {
        target("header-body")
    };
    let body = if active {
        element!(HelpDialog(control:props.help.clone())).into_any()
    } else {
        element!(View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            Text(content:"Session ID: thread-a")
            #((0..40).map(|row|element!(Text(content:format!("HISTORY_ROW_{row}")))))
        })
        .into_any()
    };
    let context = if props.clear_context_on_resize && surface.stack.viewport().0 < 48 {
        None
    } else {
        Some(agent_collaboration_tui::dialog::PinnedContext::new(
            "Feature design session · thread-a",
        ))
    };
    element!(DialogFrame(title:props.title.clone(),scroll,scroll_target,context,help_action:Some(element!(Button(id:TargetId::for_action(&action("help")),action:action("help"))).into_any())) {#(Some(body))})
}
#[derive(Props)]
struct HeaderSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    width: u16,
    height: u16,
    title: Option<String>,
    help: Arc<Mutex<Option<HelpControl>>>,
    clear_context_on_resize: bool,
}
#[component]
fn HeaderScene(props: &HeaderSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("header-screen").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let scroll = ScrollHandle::new(stack);
        let help = HelpControl::new(stack, action("help"));
        *props.help.lock().unwrap() = Some(help.clone());
        let help_run = help.open_handler(
            || HelpContent {
                context: vec![HelpContextLine {
                    label: "Session".into(),
                    value: "Feature design session · thread-a".into(),
                }],
                conventions: Vec::new(),
            },
            Handler::from(|error| panic!("{error}")),
        );
        let definitions = [
            ("close", "Close", Some("Esc")),
            ("copy-id", "Copy session ID", None),
            ("copy-path", "Copy path", None),
            ("fork", "Fork", Some("Ctrl+Shift+Enter")),
            ("resume", "Resume", None),
            ("help", "Help", Some("F1")),
        ]
        .into_iter()
        .map(|(name, title, key)| ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            id: action(name),
            title: title.into(),
            toolbar_label: None,
            keys: key.into_iter().map(|key| key.parse().unwrap()).collect(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement: if name == "help" {
                Placement::Hidden
            } else {
                Placement::Toolbar
            },
            run: if name == "help" {
                help_run.clone()
            } else {
                Handler::default()
            },
        })
        .collect::<Vec<_>>();
        let mut targets = definitions
            .iter()
            .map(|definition| {
                button_target(TargetId::for_action(&definition.id), definition)
                    .with_input(scroll.input_handler())
            })
            .collect::<Vec<_>>();
        targets.push(TargetSpec::new(target("header-body")).with_input(scroll.input_handler()));
        let presentation = match DialogPresentation::default() {
            DialogPresentation::Sized { width, .. } if props.width <= 48 || props.height < 16 => {
                DialogPresentation::Sized {
                    width,
                    height: DialogAxisSize::cells(props.height.saturating_sub(2)),
                }
            }
            presentation => presentation,
        };
        stack.open_overlay(OverlaySpec {
                kind:OverlayKind::Dialog,
                model:SurfaceModel {focus:FocusModel::new(targets).unwrap(),actions:ActionRegistry::new(ScopeId::new("header-details").unwrap(),definitions).unwrap(),primary_action:Some(action("resume"))},
                view:OverlayView::new({ let title=props.title.clone().unwrap_or_else(||"Session details".into());let clear_context_on_resize=props.clear_context_on_resize; move ||element!(HeaderPanel(title:title.clone(),help:help.clone(),scroll:scroll.clone(),clear_context_on_resize)).into_any() }),
                layout:OverlayLayout::Dialog {presentation,margins:DialogMargins {top:1,right:2,bottom:1,left:2}},
            }).unwrap();
        opened.set(true);
    }
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),Vec::new())]))) {OverlayHost})
}
#[tokio::test]
async fn standard_details_grids_keep_pinned_context_body_fact_and_all_actions() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let help = Arc::new(Mutex::new(None));
            let mut ui = element!(HeaderScene(journal, width, height,help:help.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let dialog = driver.frames.last().unwrap().round_frame_text();
                if width == 48 && height == 12 {
                    assert_eq!(
                        dialog,
                        include_str!("test_fixtures/details-48x12.txt").trim_end(),
                        "48x12 is unchanged"
                    );
                }
                if width == 120 {
                    assert_eq!(
                        dialog,
                        include_str!("test_fixtures/details-wide.txt").trim_end(),
                        "120x36 grid unchanged"
                    );
                }
                assert!(
                    dialog.contains("Session ID:"),
                    "first body fact at {width}x{height}\n{dialog}"
                );
                let rows = dialog.lines().collect::<Vec<_>>();
                let header = rows
                    .iter()
                    .position(|row| row.contains("Session details"))
                    .unwrap();
                let first = rows
                    .iter()
                    .position(|row| row.contains("Session ID:"))
                    .unwrap();
                if width <= 48 && height > 12 {
                    assert!(
                        rows[header + 1].contains("Feature"),
                        "context moves under title when three body rows fit\n{dialog}"
                    );
                    let identity = rows[header + 1..first]
                        .join("")
                        .chars()
                        .filter(|character| !character.is_whitespace() && *character != '│')
                        .collect::<String>();
                    assert!(
                        identity.contains(if width < 48 {
                            "Featuredesignsession"
                        } else {
                            "Featuredesignsession·thread-a"
                        }),
                        "typed identity fits its own row with ellipsis when needed\n{dialog}"
                    );
                    assert!(
                        dialog.contains("HISTORY_ROW_0") && dialog.contains("HISTORY_ROW_1"),
                        "at least three body rows remain\n{dialog}"
                    );
                    assert!(
                        first == header + 3,
                        "one title row, one identity row and pinned gap\n{dialog}"
                    );
                } else {
                    assert!(
                        first <= header + 2,
                        "compact or fitting header plus at most pinned gap\n{dialog}"
                    );
                }
                if width == 120 {
                    assert!(
                        rows[header].contains("Feature design session · thread-a")
                            && rows[header].contains("Help F1"),
                        "fitting header unchanged\n{dialog}"
                    );
                } else {
                    assert!(
                        !dialog.contains("Help F1"),
                        "Help hint yields before context\n{dialog}"
                    );
                    if width < 76 && height <= 12 {
                        assert!(
                            rows[header].contains('…'),
                            "clipped context has an ellipsis\n{dialog}"
                        );
                    } else if width >= 76 {
                        assert!(
                            rows[header].contains("Feature design session · thread-a"),
                            "context wins before Help hint\n{dialog}"
                        );
                    }
                }
                for title in ["Close", "Copy session ID", "Copy path", "Fork", "Resume"] {
                    assert!(
                        dialog.contains(title),
                        "{title} at {width}x{height}\n{dialog}"
                    );
                }
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(
                            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::End)),
                            true,
                        )],
                        true,
                    )
                    .await;
                let end = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    end.contains("HISTORY_ROW_39"),
                    "body remains scrollable\n{end}"
                );
                for title in ["Close", "Copy session ID", "Copy path", "Fork", "Resume"] {
                    assert!(end.contains(title), "{title} after scrolling\n{end}");
                }
                let acknowledgements = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(
                            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(1))),
                            true,
                        )],
                        true,
                    )
                    .await;
                let control = help.lock().unwrap().clone().unwrap();
                let content = control
                    .content(acknowledgements[0].snapshot.0[0].0)
                    .expect("Help is open in this dialog");
                assert!(
                    content
                        .context
                        .iter()
                        .any(|line| line.value == "Feature design session · thread-a"),
                    "full truncated identity stays reachable in supplied Help context"
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
#[tokio::test]
async fn overlong_dialog_title_truncates_with_an_ellipsis_on_one_row() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(HeaderScene(journal,width:32u16,height:24u16,title:Some("A deliberately long dialog title".to_owned())));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        let dialog = driver.frames.last().unwrap().round_frame_text();
        assert!(
            dialog.contains("A deliberately long dia…"),
            "title truncates before wrapping\n{dialog}"
        );
        assert!(
            dialog.contains("Session ID:"),
            "title keeps a body row\n{dialog}"
        );
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn help_hint_returns_when_the_live_context_disappears_on_resize() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui =
            element!(HeaderScene(journal,width:48u16,height:24u16,clear_context_on_resize:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            assert!(
                !driver
                    .frames
                    .last()
                    .unwrap()
                    .round_frame_text()
                    .contains("Help F1")
            );
            driver
                .drive(
                    &mut frames,
                    mode,
                    vec![(TerminalEvent::Resize(40, 24), true)],
                    true,
                )
                .await;
            let resized = driver.frames.last().unwrap().round_frame_text();
            assert!(
                resized.contains("Help F1"),
                "a removed context must not retain its width or wrapping state\n{resized}"
            );
            assert!(resized.contains("Session ID:"), "{resized}");
            driver
                .drive(
                    &mut frames,
                    mode,
                    vec![(TerminalEvent::Resize(48, 24), true)],
                    true,
                )
                .await;
            assert!(
                !driver
                    .frames
                    .last()
                    .unwrap()
                    .round_frame_text()
                    .contains("Help F1")
            );
        })
        .await
        .unwrap();
    }
}
