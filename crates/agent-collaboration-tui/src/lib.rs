//! Shared terminal presentation. Product state and effects remain with callers.
pub mod actions;
pub mod controls;
pub mod dialog;
pub mod foundation;
pub mod input;
pub mod surfaces;
pub use controls::typography::{
    SurfaceHeader, SurfaceHeaderProps, UiColumnHeading, UiColumnHeadingProps, UiHeading,
    UiHeadingLevel, UiHeadingProps, UiSurface, UiText, UiTextProps, UiTextRole,
};
pub use foundation::dialog_sizing::{
    DialogAxisBounds, DialogAxisSize, DialogMargins, DialogPercentage, DialogPreferredSize,
    DialogPresentation,
};

#[cfg(test)]
extern crate self as agent_collaboration_tui;
#[cfg(test)]
mod test_support;
