//! Surface arrangement only: row scrolling, passive sidecar or static details.
use super::quota_browse_rendering::{QuotaAccountListProps, render_account_list};
use super::quota_detail_panel::{QuotaDetailPanelProps, render_detail_panel};
use iocraft::prelude::*;

pub(crate) struct QuotaBodyViewProps<'a> {
    pub(crate) list: QuotaAccountListProps<'a>,
    pub(crate) panel: QuotaDetailPanelProps<'a>,
    pub(crate) sidecar: bool,
    pub(crate) show_stacked_details: bool,
    pub(crate) filter_modal_owns_input: bool,
    pub(crate) hide_list: bool,
    pub(crate) body_height: usize,
    pub(crate) stacked_details_height: usize,
}

pub(crate) fn render_quota_body(props: QuotaBodyViewProps<'_>) -> AnyElement<'static> {
    let list = props.list;
    let panel = props.panel;
    let stacked_list = if props.hide_list {
        element!(View(height:0)).into_any()
    } else {
        render_account_list(list)
    };
    if props.sidecar {
        let list_width = (list.width.saturating_sub(2) * 3 / 5)
            .max(58)
            .min(list.width.saturating_sub(44));
        let details_width = list.width.saturating_sub(list_width + 2).max(34);
        element! {
            View(width:100pct,flex_shrink:1.0_f32,min_height:0,height:if panel.interactive {Size::Length(u32::try_from(props.body_height).unwrap_or(u32::MAX))}else {Size::Auto}) {
                #(render_account_list(QuotaAccountListProps {width:list_width,..list}))
                View(width:2) {Text(content:"")}
                #(if panel.interactive&&props.filter_modal_owns_input {
                    element!(View(width:u32::try_from(details_width).unwrap_or(u32::MAX),height:u32::try_from(panel.height).unwrap_or(u32::MAX))).into_any()
                }else {render_detail_panel(QuotaDetailPanelProps {width:details_width,..panel})})
            }
        }.into_any()
    } else if props.show_stacked_details {
        element! {
            View(width:u32::try_from(list.width).unwrap_or(u32::MAX),flex_direction:FlexDirection::Column) {
                #(stacked_list)
                #(render_detail_panel(QuotaDetailPanelProps {width:list.width,height:props.stacked_details_height,..panel}))
            }
        }.into_any()
    } else {
        render_account_list(list)
    }
}
