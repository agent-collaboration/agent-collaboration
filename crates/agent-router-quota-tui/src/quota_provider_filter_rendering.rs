//! Static toolbar fixture and the existing reset paging budget.
#[cfg(test)]
use super::quota_provider_filters::QuotaAccountFilters;
use super::quota_view_configuration::QuotaInteractionMode;
#[cfg(test)]
use iocraft::prelude::*;
pub(crate) const fn quota_filter_toolbar_height(_width: usize) -> usize {
    1
}
pub(crate) fn quota_filter_controls_height(
    mode: QuotaInteractionMode,
    width: usize,
    _chooser_open: bool,
    modal_owns_input: bool,
) -> usize {
    if mode == QuotaInteractionMode::Static || modal_owns_input {
        0
    } else {
        quota_filter_toolbar_height(width)
    }
}
#[cfg(test)]
pub(crate) fn render_quota_filter_toolbar(
    filters: QuotaAccountFilters,
    visible_count: usize,
    loaded_count: usize,
    width: usize,
    _enabled: bool,
    _on_open: impl FnMut(()) + Send + Sync + 'static,
) -> AnyElement<'static> {
    element!(View(width:u32::try_from(width).unwrap_or(u32::MAX),flex_shrink:0.0_f32) {Text(content:format!("{} · Disabled: {} · {visible_count}/{loaded_count} ▾",filters.provider.label(),filters.disabled.label()))}).into_any()
}
