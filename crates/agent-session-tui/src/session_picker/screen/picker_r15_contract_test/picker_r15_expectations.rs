//! Literal test oracles and input constructors, separate from real picker observation.
use crate::session_picker::screen::picker_r15_contract_test::*;

#[derive(Clone)]
pub(crate) struct ExpectedPicker {
    pub(crate) identity: &'static str,
    pub(crate) query: String,
    pub(crate) root: &'static str,
    pub(crate) overlays: Vec<&'static str>,
    pub(crate) client: Option<&'static str>,
    pub(crate) directory: String,
    pub(crate) details: Option<&'static str>,
    pub(crate) filter_focus: bool,
    pub(crate) width: u16,
}
impl ExpectedPicker {
    pub(crate) fn initial(width: u16) -> Self {
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
pub(crate) fn key(code: KeyCode) -> ControlInput {
    ControlInput::Event(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code)))
}
pub(crate) fn ctrl(character: char) -> ControlInput {
    ControlInput::Event(ctrl_key(character))
}
pub(crate) fn pointer(target: &str, kind: MouseEventKind) -> ControlInput {
    ControlInput::Pointer {
        target: TargetId::new(target).unwrap(),
        kind,
    }
}
pub(crate) fn click(target: &str) -> ControlInput {
    pointer(target, MouseEventKind::Down(MouseButton::Left))
}
pub(crate) fn hover(target: &str) -> ControlInput {
    pointer(target, MouseEventKind::Moved)
}
pub(crate) fn contains(text: &str) -> ControlPaint {
    ControlPaint::Contains(text.into())
}
pub(crate) fn styled(
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
pub(crate) fn step(
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
pub(crate) fn open_new(state: &mut ExpectedPicker, steps: &mut Vec<ControlStep>) {
    state.overlays = vec!["dialog"];
    state.client = Some("CodexNative");
    steps.push(step(
        ctrl('n'),
        true,
        state,
        vec![contains("New session"), contains("Directory")],
    ));
}
