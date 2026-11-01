//! Quota viewport limits and presentation cadence. No component or effect dependencies.
use std::time::Duration;

pub(crate) const MIN_QUOTA_WIDTH: usize = 32;
pub(crate) const MIN_RENDER_HEIGHT: usize = 24;
pub(crate) const SIDECAR_QUOTA_WIDTH: usize = 160;
pub(crate) const NARROW_QUOTA_WIDTH: usize = 48;
pub(crate) const LIVE_QUOTA_STATUS_RELOAD_INTERVAL: Duration = Duration::from_secs(60);
pub(crate) const LIVE_QUOTA_STATUS_SPINNER_INTERVAL: Duration = Duration::from_millis(120);
pub(crate) const QUOTA_STATUS_SPINNER_TICKS: &[&str] =
    &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum QuotaInteractionMode {
    #[default]
    Interactive,
    Static,
}
