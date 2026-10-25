//! Shared terminal presentation. Product state and effects remain with callers.
mod dialog_sizing;
pub use dialog_sizing::{
    DialogAxisBounds, DialogAxisSize, DialogPercentage, DialogPreferredSize, DialogPresentation,
};
mod dialog_body_viewport;
pub use dialog_body_viewport::{DialogBodyViewport, DialogBodyViewportProps, DialogScrollKeys};
mod dialog;
mod dialog_interaction;

pub use dialog::{DialogFrame, DialogFrameProps, DialogHost, DialogHostProps, DialogMargins};
pub use dialog_interaction::{
    DialogButton, DialogButtonProps, DialogInputScope, UseDialogInteraction,
};
mod dialog_action;
pub use dialog_action::{
    DialogAction, DialogActionEmphasis, DialogActionProps, DialogKeyboardOwner,
};
mod typography;
pub use typography::{
    SurfaceHeader, SurfaceHeaderProps, UiColumnHeading, UiColumnHeadingProps, UiHeading,
    UiHeadingLevel, UiHeadingProps, UiSurface, UiText, UiTextProps, UiTextRole,
};
mod shortcut_help;
pub use shortcut_help::{ShortcutEntry, ShortcutHelp, ShortcutHelpProps, ShortcutSection};

#[cfg(test)]
mod dialog_action_keyboard_tests;
#[cfg(test)]
mod dialog_async_tests;
#[cfg(test)]
mod dialog_tests;
mod form_field;
#[cfg(test)]
mod typography_tests;
pub use form_field::{FormField, FormFieldProps, SelectableRow, SelectableRowProps};
mod search_select;
pub use search_select::{
    ChoiceAvailability, ChoiceOption, ChoiceTransition, SearchSelect, SearchSelectProps,
    SearchSelectState,
};
#[cfg(test)]
mod search_select_tests;

mod dialog_focus_reveal;

pub use dialog_focus_reveal::{FocusReveal, FocusRevealProps};

mod dialog_footer;
pub use dialog_footer::{DialogFooter, DialogFooterProps};
