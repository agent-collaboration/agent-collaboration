use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::fields::{FilterField, TextField, TextFieldState},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    },
};
use iocraft::prelude::*;
fn target() -> TargetId {
    TargetId::new("field").unwrap()
}
fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
#[derive(Props)]
struct EditorSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    filter: bool,
    disabled: bool,
}
#[component]
fn EditorScene(props: &EditorSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let value = hooks.use_state(|| TextFieldState::new("a界b"));
    let disabled = props.disabled;
    let stack = use_surface_stack(&mut hooks, |stack| SurfaceModel {
        focus: FocusModel::new(vec![
            TargetSpec::new(target())
                .with_input((if props.filter {
                    TextFieldState::filter_input_handler
                } else {
                    TextFieldState::input_handler
                })(
                    value,
                    Handler::from({
                        let journal = props.journal.clone();
                        move |_| journal.log_handler("edit")
                    }),
                ))
                .with_availability(AvailabilityFn::new(move || {
                    if disabled {
                        Availability::Unavailable {
                            reason: "Read only".into(),
                        }
                    } else {
                        Availability::Available
                    }
                })),
            TargetSpec::new(TargetId::new("other").unwrap()),
        ])
        .unwrap(),
        actions: ActionRegistry::new(
            ScopeId::new("editor").unwrap(),
            vec![ActionDef {
                risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
                toolbar_label: None,
                id: action("menu"),
                title: "Menu".into(),
                keys: vec!["F10".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| {
                    stack
                        .open_overlay(OverlaySpec {
                            kind: OverlayKind::Menu,
                            model: SurfaceModel {
                                focus: FocusModel::new(Vec::new()).unwrap(),
                                actions: ActionRegistry::new(
                                    ScopeId::new("menu").unwrap(),
                                    Vec::new(),
                                )
                                .unwrap(),
                                primary_action: None,
                            },
                            view: OverlayView::new(|| element!(Text(content:"Menu")).into_any()),
                            layout: OverlayLayout::AtCells {
                                left: 20,
                                top: 3,
                                width: 15,
                                height: 5,
                            },
                        })
                        .unwrap();
                }),
            }],
        )
        .unwrap(),
        primary_action: None,
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:50u16,height:12u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,format!("{}|{}",value.read().text(),value.read().cursor()).into_bytes())]))) {
        OverlayHost {View(width:if disabled {40}else{12},height:if disabled {2}else{1}) {#(if props.filter {element!(FilterField(id:target(),value)).into_any()}else{element!(TextField(id:target(),value)).into_any()})}}
    })
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[tokio::test]
async fn filter_escape_clears_once_and_never_quits_the_screen() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(EditorScene(journal,filter:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let acks = driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![(key(KeyCode::Esc), true), (key(KeyCode::Esc), false)],
                    true,
                    |ack| assert_eq!(ack.snapshot.0, [(SurfaceId::Screen, b"|0".to_vec())]),
                )
                .await;
            assert_eq!(acks.last().unwrap().handlers.len(), 1);
            assert!(driver.frames.last().unwrap().row_text(0).contains('/'));
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn field_hover_focus_and_disabled_reason_follow_the_declared_target() {
    use agent_collaboration_tui::foundation::theme::{SurfaceRole, TextRole, Theme};
    for disabled in [false, true] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(EditorScene(journal, disabled));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            if disabled {
                let grid = driver.frames.last().unwrap();
                assert!(
                    grid.row_text(1).contains("Read only"),
                    "{}",
                    grid.row_text(0)
                );
                assert_eq!(
                    grid.cell(0, 0).unwrap().style.foreground,
                    Some(Theme::default().foreground(TextRole::Disabled))
                );
                assert_eq!(
                    grid.cell(0, 1).unwrap().style.foreground,
                    Some(Color::AnsiValue(7)),
                    "disabled reason is on its own muted row"
                );
                driver
                    .drive_checked(
                        &mut frames,
                        InputMode::Burst,
                        vec![
                            (TerminalEvent::BracketedPaste("ignored".into()), false),
                            (key(KeyCode::Char('x')), false),
                        ],
                        false,
                        |ack| assert_eq!(ack.snapshot.0[0].1, "a界b|3".as_bytes()),
                    )
                    .await;
            } else {
                assert_eq!(
                    driver
                        .frames
                        .last()
                        .unwrap()
                        .cell(0, 0)
                        .unwrap()
                        .style
                        .background,
                    Some(Theme::default().background(SurfaceRole::Selected))
                );
                driver
                    .drive(
                        &mut frames,
                        InputMode::Burst,
                        vec![(key(KeyCode::Tab), true)],
                        true,
                    )
                    .await;
                let moved = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                    MouseEventKind::Moved,
                    1,
                    0,
                ));
                driver
                    .drive(&mut frames, InputMode::Burst, vec![(moved, true)], true)
                    .await;
                assert_eq!(
                    driver
                        .frames
                        .last()
                        .unwrap()
                        .cell(0, 0)
                        .unwrap()
                        .style
                        .background,
                    Some(Theme::default().background(SurfaceRole::Raised))
                );
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn a_long_field_keeps_the_cursor_and_adjacent_text_inside_its_viewport() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(EditorScene(journal));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(
                    TerminalEvent::BracketedPaste("abcdefghijklmnopqrst".into()),
                    true,
                )],
                true,
            )
            .await;
        let grid = driver.frames.last().unwrap();
        assert!(grid.row_text(0).contains("qrst"), "{}", grid.row_text(0));
        assert_eq!(
            (0..12)
                .filter(|column| grid.cell(*column, 0).unwrap().style.underline)
                .count(),
            1
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn opening_a_menu_blocks_the_editor_in_the_same_burst() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(EditorScene(journal));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![
                        (key(KeyCode::F(10)), true),
                        (key(KeyCode::Char('z')), false),
                        (TerminalEvent::BracketedPaste("paste".into()), false),
                    ],
                    true,
                    |ack| assert_eq!(ack.snapshot.0[0].1, "a界b|3".as_bytes()),
                )
                .await;
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn text_editor_keeps_unicode_cursor_and_edits_at_the_routed_position() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(EditorScene(journal));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let acks = driver
                .drive(
                    &mut frames,
                    mode,
                    vec![
                        (key(KeyCode::Left), true),
                        (key(KeyCode::Backspace), true),
                        (TerminalEvent::BracketedPaste("📂x".into()), true),
                        (key(KeyCode::Home), true),
                        (key(KeyCode::Delete), true),
                        (key(KeyCode::End), true),
                        (key(KeyCode::Char('!')), true),
                    ],
                    true,
                )
                .await;
            let expected = [
                "a界b|2",
                "ab|1",
                "a📂xb|3",
                "a📂xb|0",
                "📂xb|0",
                "📂xb|3",
                "📂xb!|4",
            ];
            for (ack, expected) in acks.iter().zip(expected) {
                assert_eq!(ack.snapshot.0[0].1, expected.as_bytes(), "{mode:?}");
            }
            assert_eq!(
                acks.last().unwrap().handlers.len(),
                4,
                "cursor movement must not emit value changes"
            );
            let covered = driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![
                        (key(KeyCode::F(10)), true),
                        (key(KeyCode::Char('z')), false),
                        (TerminalEvent::BracketedPaste("paste".into()), false),
                    ],
                    true,
                    |ack| assert_eq!(ack.snapshot.0[0].1, "📂xb!|4".as_bytes()),
                )
                .await;
            assert_eq!(
                covered.last().unwrap().snapshot.0[0].1,
                "📂xb!|4".as_bytes()
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
