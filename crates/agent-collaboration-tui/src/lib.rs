//! Shared terminal presentation. Product state and effects remain with callers.
pub mod actions;
pub mod controls;
pub mod dialog;
pub mod foundation;
pub mod input;
pub mod surfaces;
pub use controls::combo::search_select::{
    ChoiceAvailability, ChoiceOption, ChoiceTransition, SearchSelect, SearchSelectProps,
    SearchSelectState,
};
pub use controls::fields::legacy_form_field::{
    FormField, FormFieldProps, FormFields, FormFieldsChild, FormFieldsProps, SelectableRow,
    SelectableRowProps,
};
pub use controls::focus_reveal::{FocusReveal, FocusRevealProps};
pub use controls::form_interaction::{
    DialogButton, DialogButtonProps, DialogInputScope, UseDialogInteraction,
};
pub use controls::typography::{
    SurfaceHeader, SurfaceHeaderProps, UiColumnHeading, UiColumnHeadingProps, UiHeading,
    UiHeadingLevel, UiHeadingProps, UiSurface, UiText, UiTextProps, UiTextRole,
};
pub use dialog::dialog_action::{
    DialogAction, DialogActionEmphasis, DialogActionProps, DialogKeyboardOwner,
};
pub use dialog::dialog_body_viewport::{
    DialogBodyViewport, DialogBodyViewportProps, DialogScrollKeys,
};
pub use dialog::legacy_dialog::{DialogFrame, DialogFrameProps, DialogHost, DialogHostProps};
pub use dialog::legacy_dialog_footer::{DialogFooter, DialogFooterProps};
pub use dialog::shortcut_help::{ShortcutEntry, ShortcutHelp, ShortcutHelpProps, ShortcutSection};
pub use foundation::dialog_sizing::{
    DialogAxisBounds, DialogAxisSize, DialogMargins, DialogPercentage, DialogPreferredSize,
    DialogPresentation,
};

#[cfg(test)]
extern crate self as agent_collaboration_tui;
#[cfg(test)]
mod test_support;
