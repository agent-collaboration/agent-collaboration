use crate::{
    DialogBodyViewport, DialogInputScope, DialogMargins, DialogPresentation, UseDialogInteraction,
};
use iocraft::prelude::*;

#[derive(Props)]
pub struct DialogHostProps<'a> {
    #[iocraft(required)]
    pub width: u16,
    #[iocraft(required)]
    pub height: u16,
    pub background: Vec<AnyElement<'a>>,
    pub dialog: Option<AnyElement<'a>>,
}

/// Keeps background state mounted while preventing participating widgets from
/// acting on input. Caller-level browse dispatch must also honor its modal state.
#[component]
pub fn DialogHost<'a>(props: &mut DialogHostProps<'a>) -> impl Into<AnyElement<'a>> {
    let scope = if props.dialog.is_some() {
        DialogInputScope::Blocked
    } else {
        DialogInputScope::Active
    };
    let background = std::mem::take(&mut props.background);
    let dialog = props.dialog.take();
    element! {
        View(width: u32::from(props.width), height: u32::from(props.height), overflow: Overflow::Hidden) {
            ContextProvider(value: Context::owned(scope)) { #(background) }
            ContextProvider(value: Context::owned(DialogInputScope::Active)) { #(dialog) }
        }
    }
}

#[derive(Props)]
pub struct DialogFrameProps<'a> {
    pub presentation: DialogPresentation,
    #[iocraft(required)]
    pub width: u16,
    #[iocraft(required)]
    pub height: u16,
    #[iocraft(required)]
    pub title: String,
    pub margins: DialogMargins,
    pub padding: Option<u16>,
    pub border_style: Option<BorderStyle>,
    pub border_color: Option<Color>,
    pub scroll_keys: crate::DialogScrollKeys,
    pub keyboard_owner: crate::DialogKeyboardOwner,
    pub on_close: HandlerMut<'static, ()>,
    pub help_action: Option<AnyElement<'a>>,
    /// Caller-owned identity stays attributable while the task body scrolls.
    pub context: Vec<AnyElement<'a>>,
    pub children: Vec<AnyElement<'a>>,
    pub action_hints: Vec<AnyElement<'a>>,
    pub actions: Vec<AnyElement<'a>>,
}

/// Composable dialog chrome: fixed title/actions and a library-managed scrolling
/// body. Domain selection, commit/cancel, loading and focus belong to callers.
#[component]
pub fn DialogFrame<'a>(
    props: &mut DialogFrameProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let owns_close = props.keyboard_owner == crate::DialogKeyboardOwner::Button;
    hooks.use_dialog_events({
        let mut on_close = props.on_close.take();
        move |event| {
            if owns_close
                && matches!(
                    event,
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Esc,
                        kind: KeyEventKind::Press,
                        ..
                    })
                )
            {
                on_close(());
            }
        }
    });
    let left = props.margins.left.min(props.width);
    let right = props.margins.right.min(props.width.saturating_sub(left));
    let top = props.margins.top.min(props.height);
    let bottom = props.margins.bottom.min(props.height.saturating_sub(top));
    let available_width = props.width.saturating_sub(left + right);
    let available_height = props.height.saturating_sub(top + bottom);
    let ((frame_width, width_limit), (frame_height, height_limit)) = props
        .presentation
        .resolve(available_width, available_height);
    let children = std::mem::take(&mut props.children);
    let actions = std::mem::take(&mut props.actions);
    let action_hints = std::mem::take(&mut props.action_hints);
    let help_action = props.help_action.take();
    let context = std::mem::take(&mut props.context);
    let context_gap = u16::from(!context.is_empty() && height_limit >= 16);
    let has_actions = !actions.is_empty() || !action_hints.is_empty();
    let padding = props.padding.unwrap_or(u16::from(height_limit >= 16));
    let content_height = matches!(
        props.presentation,
        DialogPresentation::Sized {
            height: crate::DialogAxisSize::Content { .. },
            ..
        }
    );
    element! {
        View(
            position: Position::Absolute, top: 0, left: 0,
            width: u32::from(props.width), height: u32::from(props.height),
            padding_left: left, padding_right: right, padding_top: top, padding_bottom: bottom,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            overflow: Overflow::Hidden,
        ) {
            View(
                width: frame_width, height: frame_height,
                max_width: u32::from(width_limit), max_height: u32::from(height_limit),
                flex_direction: FlexDirection::Column, overflow: Overflow::Hidden,
                border_style: props.border_style.unwrap_or(BorderStyle::Round),
                border_color: props.border_color.unwrap_or(Color::Cyan),
                background_color: crate::UiSurface::Base.color(), padding,
                flex_shrink: 0.0_f32,
            ) {
                crate::SurfaceHeader(title: props.title.clone(), help_action)
                View(flex_direction: FlexDirection::Column, flex_shrink: 0.0_f32, margin_bottom: context_gap) { #(context) }
                DialogBodyViewport(scroll_keys: props.scroll_keys, children)
                #((!content_height).then(|| element!(View(flex_grow: 1.0_f32))))
                #(has_actions.then(|| element! {
                    crate::DialogFooter(children:actions,hints:action_hints)
                }))
            }
        }
    }
}
