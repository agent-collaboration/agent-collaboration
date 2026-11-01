use crate::{UiTextRole, UseDialogInteraction};
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DialogActionEmphasis {
    #[default]
    Primary,
    Secondary,
}

/// A modal may own modified Enter and focus navigation as one state transition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DialogKeyboardOwner {
    #[default]
    Button,
    Caller,
}

#[derive(Props)]
pub struct DialogActionProps {
    #[iocraft(required)]
    pub label: String,
    pub has_focus: bool,
    pub enabled: Option<bool>,
    pub emphasis: DialogActionEmphasis,
    pub keyboard_owner: DialogKeyboardOwner,
    pub handler: HandlerMut<'static, ()>,
}

/// Native activation semantics with dialog input scope and viewport clipping.
#[component]
pub fn DialogAction(props: &mut DialogActionProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let enabled = hooks.dialog_input_enabled() && props.enabled.unwrap_or(true);
    let mut handler = props.handler.take();
    let (color, weight) = if !enabled {
        (UiTextRole::Disabled.color(), Weight::Normal)
    } else if props.has_focus {
        (Color::Yellow, Weight::Bold)
    } else {
        match props.emphasis {
            DialogActionEmphasis::Primary => (Color::White, Weight::Bold),
            DialogActionEmphasis::Secondary => (Color::Grey, Weight::Normal),
        }
    };
    element! {
        crate::DialogButton(has_focus: enabled && props.has_focus && props.keyboard_owner == DialogKeyboardOwner::Button, handler: move |_| {
            if enabled { handler(()); }
        }) {
            Text(content: props.label.clone(), color, weight,
                decoration: if enabled && props.has_focus { TextDecoration::Underline } else { TextDecoration::None })
        }
    }
}
