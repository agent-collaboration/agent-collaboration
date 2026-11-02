//! An editor model driven only by its declared target's routed input.
use crate::{
    actions::Availability,
    foundation::theme::{SurfaceRole, TextRole},
    surfaces::{
        focus_model::{InputUse, TargetId, TargetInputHandler},
        hit_regions::{HitRegionKind, use_target_geometry},
        surface_handle::SurfaceContext,
    },
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

#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct EditorWindow {
    first_visible_character: usize,
}

#[derive(Default)]
struct EditorWindowText {
    before: String,
    cursor: String,
    after: String,
}

impl EditorWindow {
    fn reveal_cursor(
        &mut self,
        value: &TextFieldState,
        width: u16,
        focused: bool,
    ) -> EditorWindowText {
        let width = usize::from(width);
        if width == 0 {
            return EditorWindowText::default();
        }
        let characters = value.text.chars().collect::<Vec<_>>();
        let mut cursor = characters
            .get(value.cursor)
            .map_or(" ".to_owned(), char::to_string);
        if cursor.width() == 0 || cursor.width() > width {
            cursor = " ".to_owned();
        }
        let whole_text_fits =
            value.text.width() + usize::from(value.cursor == characters.len()) <= width;
        let mut start = if focused && !whole_text_fits {
            self.first_visible_character.min(value.cursor)
        } else {
            0
        };
        let mut before = characters
            .iter()
            .skip(start)
            .take(value.cursor - start)
            .collect::<String>();
        if focused && before.width() + cursor.width() > width {
            start = value.cursor;
            before.clear();
            for (index, character) in characters.iter().enumerate().take(value.cursor).rev() {
                let candidate = format!("{character}{before}");
                if candidate.width() + cursor.width() > width {
                    break;
                }
                before = candidate;
                start = index;
            }
        }
        if focused {
            self.first_visible_character = start;
        }
        before = fit_editor_text(before.chars(), width);
        let remaining = width.saturating_sub(before.width());
        if cursor.width() > remaining {
            cursor.clear();
        }
        let after = fit_editor_text(
            characters.iter().skip(value.cursor + 1).copied(),
            remaining.saturating_sub(cursor.width()),
        );
        EditorWindowText {
            before,
            cursor,
            after,
        }
    }
}

fn fit_editor_text(characters: impl Iterator<Item = char>, width: usize) -> String {
    let mut visible = String::new();
    for character in characters {
        visible.push(character);
        if visible.width() > width {
            visible.pop();
            break;
        }
    }
    visible
}
#[derive(Props)]
pub(crate) struct EditorLineProps {
    pub value: TextFieldState,
    pub focused: bool,
    pub disabled: bool,
    pub placeholder: String,
    pub available_width: Option<u16>,
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
    let available_width = props.available_width.unwrap_or(width.get());
    let mut window = hooks.use_state(EditorWindow::default);
    let mut next = window.get();
    let EditorWindowText {
        before,
        cursor,
        after,
    } = next.reveal_cursor(&props.value, available_width, props.focused);
    if next != window.get() {
        window.set(next);
    }
    let placeholder = fit_editor_text(
        props.placeholder.chars(),
        usize::from(available_width).saturating_sub(1),
    );
    let role = if props.disabled {
        TextRole::Disabled
    } else {
        TextRole::Text
    };
    element!(View(width:100pct,min_width:0,height:1,flex_grow:1.0_f32,flex_shrink:1.0_f32,overflow:Overflow::Hidden) {
        View(flex_shrink:0.0_f32) {
            Text(content:before,color:theme.foreground(role),wrap:TextWrap::NoWrap)
            Text(content:cursor,color:theme.foreground(if props.focused&&!props.disabled {TextRole::Focused}else{role}),decoration:if props.focused {TextDecoration::Underline}else{TextDecoration::None},wrap:TextWrap::NoWrap)
            Text(content:after,color:theme.foreground(role),wrap:TextWrap::NoWrap)
            #((props.value.text.is_empty()&&!props.focused).then(||element!(Text(content:placeholder,color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap))))
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
    let focused = context.is_focused(&props.id);
    let value = props.value.read().clone();
    element!(View(width:100pct,min_height:1,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden,background_color:theme.background(if focused {SurfaceRole::Selected}else if context.is_hovered(&props.id){SurfaceRole::Raised}else{SurfaceRole::Base})) {
        EditorLine(value,focused,disabled,placeholder:props.placeholder.clone())
        #(match availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(element!(super::wrapped_reason::WrappedReason(content:reason,color:theme.foreground(TextRole::Muted))))})
    })
}
#[component]
pub fn FilterField(props: &TextFieldProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    element!(View(width:100pct,column_gap:1) {
        View(flex_shrink:0.0_f32) {Text(content:"/",color:context.theme().foreground(TextRole::Muted))}
        View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0) {TextField(id:props.id.clone(),value:props.value,placeholder:props.placeholder.clone())}
    })
}

#[cfg(test)]
#[path = "text_field_test.rs"]
mod tests;

#[cfg(test)]
#[path = "editor_window_fixture.rs"]
mod editor_window_fixture;

#[cfg(test)]
#[path = "editor_window_test.rs"]
mod editor_window_test;
