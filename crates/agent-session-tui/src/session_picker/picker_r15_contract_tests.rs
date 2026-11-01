//! Independent R15 outcomes for the production Sessions picker instances.
use super::picker_r15_instance::PickerR15Instance;
use super::*;
use crate::picker_control_test_support::{
    control_kind_contract::{
        ControlInput, ControlKind, ControlKindContract, ControlPaint, ControlStep,
        assert_control_kind_matrix,
    },
    event_driver::SurfaceSnapshot,
    frame_grid::FrameStyle,
    grids::STANDARD_GRIDS,
};
use agent_collaboration_tui::surfaces::{focus_model::TargetId, surface_stack::SurfaceId};

#[derive(Clone)]
struct ExpectedPicker {
    identity: &'static str,
    query: String,
    root: &'static str,
    overlays: Vec<&'static str>,
    client: Option<&'static str>,
    directory: String,
    details: Option<&'static str>,
    filter_focus: bool,
    width: u16,
}
impl ExpectedPicker {
    fn initial(width: u16) -> Self {
        Self {
            identity: "r15-a",
            query: String::new(),
            root: "Cwd",
            overlays: vec![],
            client: None,
            directory: "/repo/project-a".into(),
            details: None,
            filter_focus: false,
            width,
        }
    }
    fn snapshot(&self) -> SurfaceSnapshot {
        let bytes=serde_json::to_vec(&serde_json::json!({
            "identity":format!("LocalCodex({:?})",self.identity),"query":self.query,"root":self.root,
            "overlays":self.overlays,
            "new":self.client.map(|client|serde_json::json!({"client":client,"directory":self.directory})),
            "start_reason":if self.client==Some("ClaudeNativeBackground") {Some("Selected source client capabilities have not been checked")} else {None},"details":self.details,"scope":self.width==120,"new_button":self.width!=32,
            "filter_focus":self.filter_focus,"client_focus":self.client.is_some()&&self.overlays.last()==Some(&"dialog"),
        })).unwrap();
        SurfaceSnapshot(vec![(SurfaceId::Screen, bytes)])
    }
}
fn key(code: KeyCode) -> ControlInput {
    ControlInput::Event(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code)))
}
fn ctrl(character: char) -> ControlInput {
    ControlInput::Event(ctrl_key(character))
}
fn pointer(target: &str, kind: MouseEventKind) -> ControlInput {
    ControlInput::Pointer {
        target: TargetId::new(target).unwrap(),
        kind,
    }
}
fn click(target: &str) -> ControlInput {
    pointer(target, MouseEventKind::Down(MouseButton::Left))
}
fn hover(target: &str) -> ControlInput {
    pointer(target, MouseEventKind::Moved)
}
fn contains(text: &str) -> ControlPaint {
    ControlPaint::Contains(text.into())
}
fn styled(
    text: &str,
    foreground: Color,
    background: Color,
    bold: bool,
    underline: bool,
) -> ControlPaint {
    ControlPaint::Styled {
        text: text.into(),
        style: FrameStyle {
            foreground: Some(foreground),
            background: Some(background),
            bold,
            underline,
            ..Default::default()
        },
    }
}
fn step(
    input: ControlInput,
    redraw: bool,
    state: &ExpectedPicker,
    paint: Vec<ControlPaint>,
) -> ControlStep {
    ControlStep {
        input,
        redraw,
        expected_snapshot: state.snapshot(),
        expected_handlers: vec![],
        paint,
    }
}
fn open_new(state: &mut ExpectedPicker, steps: &mut Vec<ControlStep>) {
    state.overlays = vec!["dialog"];
    state.client = Some("CodexNative");
    steps.push(step(
        ctrl('n'),
        true,
        state,
        vec![contains("New session"), contains("Directory")],
    ));
}
fn contract(kind: ControlKind, viewport: (u16, u16)) -> ControlKindContract {
    let mut state = ExpectedPicker::initial(viewport.0);
    let mut steps = Vec::new();
    let mut initial = vec![contains("Menu"), contains("2/2")];
    match kind {
        ControlKind::Button => {
            initial.push(styled(
                "Menu",
                Color::AnsiValue(15),
                Color::Rgb {
                    r: 39,
                    g: 44,
                    b: 52,
                },
                false,
                false,
            ));
            steps.push(step(
                hover("action:menu"),
                true,
                &state,
                vec![styled(
                    "Menu",
                    Color::AnsiValue(15),
                    Color::Rgb {
                        r: 48,
                        g: 54,
                        b: 65,
                    },
                    false,
                    false,
                )],
            ));
            steps.push(step(
                key(KeyCode::Tab),
                true,
                &state,
                vec![styled(
                    "Menu",
                    Color::AnsiValue(11),
                    Color::Rgb {
                        r: 48,
                        g: 54,
                        b: 65,
                    },
                    true,
                    true,
                )],
            ));
            state.overlays = vec!["menu"];
            steps.push(step(
                click("action:menu"),
                true,
                &state,
                vec![contains("New session")],
            ));
            for _ in 0..17 {
                steps.push(step(key(KeyCode::Down), true, &state, vec![]));
            }
            state.overlays = vec!["dialog"];
            state.client = Some("ClaudeNativeBackground");
            steps.push(step(
                key(KeyCode::Enter),
                true,
                &state,
                vec![
                    contains("Start"),
                    styled(
                        "Start",
                        Color::Rgb {
                            r: 166,
                            g: 175,
                            b: 189,
                        },
                        Color::Rgb {
                            r: 39,
                            g: 44,
                            b: 52,
                        },
                        false,
                        false,
                    ),
                ],
            ));
            let mut disabled = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
            disabled.modifiers = KeyModifiers::CONTROL;
            steps.push(step(
                ControlInput::Event(TerminalEvent::Key(disabled)),
                false,
                &state,
                vec![],
            ));
            steps.push(step(click("action:new-start"), false, &state, vec![]));
        }
        ControlKind::TextField => {
            // FilterField owns a real TextField child; Directory is read-only.
            steps.push(step(
                hover("session-filter"),
                true,
                &state,
                vec![contains("Filter…")],
            ));
            state.filter_focus = true;
            steps.push(step(
                click("session-filter"),
                true,
                &state,
                vec![ControlPaint::Absent("Filter…".into())],
            ));
            state.query = "Beta".into();
            state.identity = "r15-b";
            steps.push(step(
                ControlInput::Event(TerminalEvent::BracketedPaste("Beta".into())),
                true,
                &state,
                vec![contains("1/2")],
            ));
            steps.push(step(key(KeyCode::Left), true, &state, vec![]));
            steps.push(step(key(KeyCode::Right), true, &state, vec![]));
            state.query = "Bet".into();
            steps.push(step(
                key(KeyCode::Backspace),
                true,
                &state,
                vec![contains("1/2")],
            ));
            state.query.clear();
            steps.push(step(key(KeyCode::Esc), true, &state, vec![contains("2/2")]));
            state.filter_focus = false;
            state.overlays = vec!["dialog"];
            state.details = Some("r15-b");
            steps.push(step(
                key(KeyCode::Enter),
                true,
                &state,
                vec![contains("Session ID:")],
            ));
        }
        ControlKind::FilterField => {
            initial.push(contains("Filter…"));
            initial.push(styled(
                "Filter…",
                Color::AnsiValue(7),
                Color::Rgb {
                    r: 39,
                    g: 44,
                    b: 52,
                },
                false,
                false,
            ));
            steps.push(step(
                hover("session-filter"),
                true,
                &state,
                vec![styled(
                    "Filter…",
                    Color::AnsiValue(7),
                    Color::Rgb {
                        r: 48,
                        g: 54,
                        b: 65,
                    },
                    false,
                    false,
                )],
            ));
            state.filter_focus = true;
            steps.push(step(
                click("session-filter"),
                true,
                &state,
                vec![ControlPaint::Absent("Filter…".into())],
            ));
            state.query = "Beta".into();
            state.identity = "r15-b";
            steps.push(step(
                ControlInput::Event(TerminalEvent::BracketedPaste("Beta".into())),
                true,
                &state,
                vec![contains("1/2"), contains("Beta")],
            ));
            state.query.clear();
            state.identity = "r15-b";
            steps.push(step(key(KeyCode::Esc), true, &state, vec![contains("2/2")]));
        }
        ControlKind::ComboBox => {
            open_new(&mut state, &mut steps);
            steps.push(step(
                hover("new-client"),
                false,
                &state,
                vec![contains("Codex")],
            ));
            state.overlays.push("dropdown:new-client");
            steps.push(step(
                click("new-client"),
                true,
                &state,
                vec![contains("Claude Code")],
            ));
            steps.push(step(
                key(KeyCode::Down),
                true,
                &state,
                vec![styled(
                    "Claude Code",
                    Color::Rgb {
                        r: 166,
                        g: 175,
                        b: 189,
                    },
                    Color::Rgb {
                        r: 52,
                        g: 59,
                        b: 70,
                    },
                    false,
                    false,
                )],
            ));
            steps.push(step(key(KeyCode::Enter), false, &state, vec![]));
            steps.push(step(
                click("10:new-client:option:client:ClaudeNativeForeground"),
                false,
                &state,
                vec![],
            ));
            state.overlays.pop();
            steps.push(step(
                key(KeyCode::Esc),
                true,
                &state,
                vec![contains("Codex")],
            ));
            state.overlays.push("dropdown:new-client");
            steps.push(step(key(KeyCode::Enter), true, &state, vec![]));
            steps.push(step(
                hover("10:new-client:option:client:ClaudeNativeForeground"),
                true,
                &state,
                vec![],
            ));
            steps.push(step(
                hover("10:new-client:option:client:CodexNative"),
                true,
                &state,
                vec![],
            ));
            state.overlays.pop();
            steps.push(step(
                click("10:new-client:option:client:CodexNative"),
                true,
                &state,
                vec![contains("Codex")],
            ));
        }
        ControlKind::SegmentedSwitch => {
            if viewport.0 == 120 {
                initial.push(contains("This folder"));
                initial.push(styled(
                    "This folder",
                    Color::AnsiValue(11),
                    Color::Rgb {
                        r: 39,
                        g: 44,
                        b: 52,
                    },
                    true,
                    false,
                ));
                steps.push(step(
                    hover("action:scope-all"),
                    true,
                    &state,
                    vec![styled(
                        "All",
                        Color::AnsiValue(15),
                        Color::Rgb {
                            r: 48,
                            g: 54,
                            b: 65,
                        },
                        false,
                        false,
                    )],
                ));
                state.root = "Any";
                steps.push(step(
                    click("action:scope-all"),
                    true,
                    &state,
                    vec![styled(
                        "All",
                        Color::AnsiValue(11),
                        Color::Rgb {
                            r: 48,
                            g: 54,
                            b: 65,
                        },
                        true,
                        true,
                    )],
                ));
                state.root = "Cwd";
                steps.push(step(ctrl('s'), true, &state, vec![contains("This folder")]));
            } else {
                // Narrow projection hides the real switch; keyboard setting remains real.
                initial.push(ControlPaint::Absent("This folder".into()));
                state.root = "Repo";
                // Both records are in this folder and repo: hidden setting changes
                // its domain value without changing this serialized frame.
                steps.push(step(ctrl('s'), false, &state, vec![contains("2/2")]));
            }
        }
        ControlKind::ScrollingList => {
            initial.push(contains("Alpha"));
            initial.push(styled(
                "❯",
                Color::AnsiValue(11),
                Color::Rgb {
                    r: 52,
                    g: 59,
                    b: 70,
                },
                false,
                false,
            ));
            state.identity = "r15-b";
            steps.push(step(
                key(KeyCode::Down),
                true,
                &state,
                vec![contains("Beta")],
            ));
            state.identity = "r15-a";
            steps.push(step(
                key(KeyCode::Up),
                true,
                &state,
                vec![contains("Alpha")],
            ));
            state.identity = "r15-b";
            steps.push(step(
                hover("13:sessions-list:row:localCodex:r15-b"),
                true,
                &state,
                vec![contains("Beta")],
            ));
            steps.push(step(
                click("13:sessions-list:row:localCodex:r15-b"),
                false,
                &state,
                vec![],
            ));
        }
        ControlKind::FormField => {
            state.overlays = vec!["dialog"];
            state.details = Some("r15-a");
            steps.push(step(
                key(KeyCode::Enter),
                true,
                &state,
                vec![styled(
                    "Session ID:",
                    Color::Rgb {
                        r: 156,
                        g: 188,
                        b: 251,
                    },
                    Color::Rgb {
                        r: 39,
                        g: 44,
                        b: 52,
                    },
                    false,
                    false,
                )],
            ));
            let mut copy = step(
                click("action:details-copy-id"),
                true,
                &state,
                vec![contains("Session ID:")],
            );
            copy.expected_handlers = vec!["copy:r15-a".into()];
            steps.push(copy);
            state.overlays.clear();
            state.details = None;
            let mut close = step(key(KeyCode::Esc), true, &state, vec![contains("Alpha")]);
            close.expected_handlers = vec!["copy:r15-a".into()];
            steps.push(close);
        }
        ControlKind::DialogKeyHints => {
            open_new(&mut state, &mut steps);
            steps.last_mut().unwrap().paint.push(styled(
                "Ctrl+Enter",
                Color::AnsiValue(7),
                Color::Rgb {
                    r: 39,
                    g: 44,
                    b: 52,
                },
                false,
                false,
            ));
            state.overlays.clear();
            state.client = None;
            steps.push(step(
                click("action:dialog-close"),
                true,
                &state,
                vec![contains("2/2")],
            ));
            open_new(&mut state, &mut steps);
            state.overlays.clear();
            state.client = None;
            steps.push(step(key(KeyCode::Esc), true, &state, vec![contains("2/2")]));
        }
    }
    ControlKindContract {
        screen: "Sessions",
        kind,
        viewport,
        initial_paint: initial,
        steps,
    }
}
#[tokio::test]
async fn r15_actual_picker_control_instances_cover_all_kinds_and_grids() {
    let contracts = STANDARD_GRIDS
        .into_iter()
        .flat_map(|viewport| {
            [
                ControlKind::Button,
                ControlKind::TextField,
                ControlKind::FilterField,
                ControlKind::ComboBox,
                ControlKind::SegmentedSwitch,
                ControlKind::ScrollingList,
                ControlKind::FormField,
                ControlKind::DialogKeyHints,
            ]
            .map(|kind| contract(kind, viewport))
        })
        .collect();
    assert_control_kind_matrix(contracts, |instance, journal, observed| {
        element!(PickerR15Instance(instance, journal, observed))
    })
    .await;
}
