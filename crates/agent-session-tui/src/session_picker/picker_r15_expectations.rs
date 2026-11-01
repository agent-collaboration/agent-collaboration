//! Literal test oracles and input constructors, separate from real picker observation.
use super::*;

#[derive(Clone)]
pub(super) struct ExpectedPicker {
    pub(super) identity: &'static str,
    pub(super) query: String,
    pub(super) root: &'static str,
    pub(super) overlays: Vec<&'static str>,
    pub(super) client: Option<&'static str>,
    pub(super) directory: String,
    pub(super) details: Option<&'static str>,
    pub(super) filter_focus: bool,
    pub(super) width: u16,
}
impl ExpectedPicker {
    pub(super) fn initial(width: u16) -> Self {
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
pub(super) fn key(code: KeyCode) -> ControlInput {
    ControlInput::Event(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code)))
}
pub(super) fn ctrl(character: char) -> ControlInput {
    ControlInput::Event(ctrl_key(character))
}
pub(super) fn pointer(target: &str, kind: MouseEventKind) -> ControlInput {
    ControlInput::Pointer {
        target: TargetId::new(target).unwrap(),
        kind,
    }
}
pub(super) fn click(target: &str) -> ControlInput {
    pointer(target, MouseEventKind::Down(MouseButton::Left))
}
pub(super) fn hover(target: &str) -> ControlInput {
    pointer(target, MouseEventKind::Moved)
}
pub(super) fn contains(text: &str) -> ControlPaint {
    ControlPaint::Contains(text.into())
}
pub(super) fn styled(
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
pub(super) fn step(
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
pub(super) fn open_new(state: &mut ExpectedPicker, steps: &mut Vec<ControlStep>) {
    state.overlays = vec!["dialog"];
    state.client = Some("CodexNative");
    steps.push(step(
        ctrl('n'),
        true,
        state,
        vec![contains("New session"), contains("Directory")],
    ));
}
