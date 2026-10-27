//! CLI-only terminal presentation helpers.

use iocraft::prelude::*;

pub(crate) mod host;
pub(crate) mod quota;

/// Marker type for the CLI presentation layer's iocraft boundary.
#[expect(
    dead_code,
    reason = "The marker records the CLI boundary that owns the iocraft dependency."
)]
pub(crate) struct TerminalPresentationLayer {
    _terminal_color: Option<Color>,
}

impl TerminalPresentationLayer {
    /// Creates a presentation-layer marker without enabling terminal output.
    #[expect(
        dead_code,
        reason = "The boundary marker needs no terminal output or runtime instance."
    )]
    pub(crate) const fn new() -> Self {
        Self {
            _terminal_color: None,
        }
    }
}
