//! Surface arrangement only: row scrolling, passive sidecar or static details.
use super::quota_browse_rendering::{QuotaAccountListProps, render_account_list};
use super::quota_detail_panel::{QuotaDetailPanelProps, render_detail_panel};
use iocraft::prelude::*;

pub(super) struct QuotaBodyViewProps<'a> {
    pub list: QuotaAccountListProps<'a>,
    pub panel: QuotaDetailPanelProps<'a>,
    pub sidecar: bool,
    pub show_stacked_details: bool,
    pub filter_modal_owns_input: bool,
    pub hide_list: bool,
    pub body_height: usize,
    pub stacked_details_height: usize,
}

pub(super) fn render_quota_body(props: QuotaBodyViewProps<'_>) -> AnyElement<'static> {
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
            View(width:100pct,flex_shrink:1.0_f32,min_height:0,height:if panel.interactive {Size::Length(props.body_height as u32)}else {Size::Auto}) {
                #(render_account_list(QuotaAccountListProps {width:list_width,..list}))
                View(width:2) {Text(content:"")}
                #(if panel.interactive&&props.filter_modal_owns_input {
                    element!(View(width:details_width as u32,height:panel.height as u32)).into_any()
                }else {render_detail_panel(QuotaDetailPanelProps {width:details_width,..panel})})
            }
        }.into_any()
    } else if props.show_stacked_details {
        element! {
            View(width:list.width as u32,flex_direction:FlexDirection::Column) {
                #(stacked_list)
                #(render_detail_panel(QuotaDetailPanelProps {width:list.width,height:props.stacked_details_height,..panel}))
            }
        }.into_any()
    } else {
        render_account_list(list)
    }
}
