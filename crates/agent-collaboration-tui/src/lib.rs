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

#[cfg(any(test, feature = "test-support"))]
extern crate self as agent_collaboration_tui;
// Keep the owner’s literal test context separate from the feature-built fixtures.
#[cfg(test)]
pub mod test_support;
#[cfg(all(not(test), feature = "test-support"))]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "These assertion-based fixtures are test code behind a non-default test-support feature; the shipping closure must exclude it."
)]
pub mod test_support;
