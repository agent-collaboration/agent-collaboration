//! Printed output and the interactive control tree share quota facts, not input paths.
pub(crate) use super::quota_status_props::QuotaStatusComponentProps;
use super::quota_view_configuration::QuotaInteractionMode;
use iocraft::prelude::*;

#[component]
pub(crate) fn QuotaStatusComponent(
    props: &mut QuotaStatusComponentProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    if props.interaction_mode == QuotaInteractionMode::Static {
        super::quota_static_component::render_static_quota_status(props, hooks)
    } else {
        super::quota_interactive_component::render_interactive_quota_status(props, hooks)
    }
}
