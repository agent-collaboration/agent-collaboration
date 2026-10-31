//! An editor model driven only by its declared target's routed input.
use crate::{
    actions::Availability,
    focus_model::{InputUse, TargetId, TargetInputHandler},
    hit_regions::{HitRegionKind, use_target_geometry},
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole},
};
use iocraft::{ComponentDrawer, Hook, prelude::*};
use unicode_width::UnicodeWidthStr;
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextFieldState {
    text: String,
    cursor: usize,
}
impl TextFieldState {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
    pub fn handle_event(&mut self, event: &TerminalEvent) -> InputUse {
        let mut characters = self.text.chars().collect::<Vec<_>>();
        let insert = match event {
            TerminalEvent::BracketedPaste(value) => Some(value.clone()),
            TerminalEvent::Key(key)
                if key.kind != KeyEventKind::Release
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                match key.code {
                    KeyCode::Left => {
                        self.cursor = self.cursor.saturating_sub(1);
                        return InputUse::Used;
                    }
                    KeyCode::Right => {
                        self.cursor = (self.cursor + 1).min(characters.len());
                        return InputUse::Used;
                    }
                    KeyCode::Home => {
                        self.cursor = 0;
                        return InputUse::Used;
                    }
                    KeyCode::End => {
                        self.cursor = characters.len();
                        return InputUse::Used;
                    }
                    KeyCode::Backspace => {
                        if self.cursor > 0 {
                            self.cursor -= 1;
                            characters.remove(self.cursor);
                        }
                        None
                    }
                    KeyCode::Delete => {
                        if self.cursor < characters.len() {
                            characters.remove(self.cursor);
                        }
                        None
                    }
                    KeyCode::Char(character) if !character.is_control() => {
                        Some(character.to_string())
                    }
                    _ => return InputUse::Unused,
                }
            }
            _ => return InputUse::Unused,
        };
        if let Some(insert) = insert {
            let insert = insert
                .chars()
                .filter(|character| !character.is_control())
                .collect::<Vec<_>>();
            let length = insert.len();
            characters.splice(self.cursor..self.cursor, insert);
            self.cursor += length;
        }
        self.text = characters.into_iter().collect();
        InputUse::Used
    }
    pub fn input_handler(state: State<Self>, on_change: Handler<String>) -> TargetInputHandler {
        TargetInputHandler::new(move |event| {
            let previous = state.read().clone();
            let mut next = previous.clone();
            let used = next.handle_event(&event);
            if used != InputUse::Unused && next != previous {
                let changed = next.text != previous.text;
                let text = next.text.clone();
                let mut state = state;
                state.set(next);
                if changed {
                    on_change(text);
                }
            }
            used
        })
    }
    pub fn filter_input_handler(
        state: State<Self>,
        on_change: Handler<String>,
    ) -> TargetInputHandler {
        let editor = Self::input_handler(state, on_change.clone());
        TargetInputHandler::new(move |event| {
            if matches!(&event,TerminalEvent::Key(key) if key.code==KeyCode::Esc&&key.kind==KeyEventKind::Press)
                && !state.read().text.is_empty()
            {
                let mut state = state;
                state.set(Self::default());
                on_change(String::new());
                InputUse::Used
            } else {
                editor.call(event)
            }
        })
    }
}
struct EditorMeasure(State<u16>);
impl Hook for EditorMeasure {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if self.0.get() != drawer.size().width {
            self.0.set(drawer.size().width);
        }
    }
}
#[derive(Props)]
pub(crate) struct EditorLineProps {
    pub value: TextFieldState,
    pub focused: bool,
    pub disabled: bool,
    pub placeholder: String,
}
#[component]
pub(crate) fn EditorLine(
    props: &EditorLineProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let width = hooks.use_state(|| 0u16);
    hooks.use_hook(|| EditorMeasure(width)).0 = width;
    let characters = props.value.text.chars().collect::<Vec<_>>();
    let before = characters
        .iter()
        .take(props.value.cursor)
        .collect::<String>();
    let cursor = characters
        .get(props.value.cursor)
        .map_or(" ".to_owned(), char::to_string);
    let after = characters
        .iter()
        .skip(props.value.cursor + 1)
        .collect::<String>();
    let offset = if props.focused {
        UnicodeWidthStr::width(before.as_str())
            .saturating_sub(usize::from(width.get().saturating_sub(1)))
    } else {
        0
    };
    let role = if props.disabled {
        TextRole::Disabled
    } else {
        TextRole::Text
    };
    element!(View(width:100pct,min_width:0,height:1,flex_grow:1.0_f32,flex_shrink:1.0_f32,overflow:Overflow::Hidden) {
        View(position:Position::Relative,left:-i32::try_from(offset).unwrap_or(i32::MAX),flex_shrink:0.0_f32) {
            Text(content:before,color:theme.foreground(role),wrap:TextWrap::NoWrap)
            Text(content:cursor,color:theme.foreground(if props.focused {TextRole::Focused}else{role}),decoration:if props.focused {TextDecoration::Underline}else{TextDecoration::None},wrap:TextWrap::NoWrap)
            Text(content:after,color:theme.foreground(role),wrap:TextWrap::NoWrap)
            #((props.value.text.is_empty()&&!props.focused).then(||element!(Text(content:props.placeholder.clone(),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap))))
        }
    })
}
#[derive(Props)]
pub struct TextFieldProps {
    #[iocraft(required)]
    pub id: TargetId,
    #[iocraft(required)]
    pub value: State<TextFieldState>,
    pub placeholder: String,
}
#[component]
pub fn TextField(props: &TextFieldProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    use_target_geometry(&mut hooks, context, props.id.clone(), HitRegionKind::Target);
    let theme = context.theme();
    let availability = context
        .stack
        .snapshot()
        .surface(context.surface)
        .and_then(|surface| {
            surface
                .focus
                .targets()
                .iter()
                .find(|target| target.id == props.id)
        })
        .map(|target| target.availability.get())
        .unwrap_or(Availability::Unavailable {
            reason: "Unavailable".into(),
        });
    let disabled = !availability.is_available();
    let focused = context.is_focused(&props.id) && !disabled;
    let value = props.value.read().clone();
    element!(View(width:100pct,min_height:1,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden,background_color:theme.background(if focused {SurfaceRole::Selected}else if context.is_hovered(&props.id){SurfaceRole::Raised}else{SurfaceRole::Base})) {
        EditorLine(value,focused,disabled,placeholder:props.placeholder.clone())
        #(match availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(element!(View(width:100pct,flex_shrink:0.0_f32) {Text(content:reason,wrap:TextWrap::Wrap,color:theme.foreground(TextRole::Muted))}))})
    })
}
#[component]
pub fn FilterField(props: &TextFieldProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    element!(View(width:100pct,column_gap:1) {Text(content:"/",color:context.theme().foreground(TextRole::Muted)) TextField(id:props.id.clone(),value:props.value,placeholder:props.placeholder.clone())})
}
