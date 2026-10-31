//! The printed quota format has no terminal subscription or overlay state.
use super::{
    quota_provider_filters::QuotaFilterState,
    quota_status_props::QuotaStatusComponentProps,
    quota_view_configuration::QuotaInteractionMode,
    quota_view_configuration::{
        MIN_QUOTA_WIDTH, MIN_RENDER_HEIGHT, NARROW_QUOTA_WIDTH, SIDECAR_QUOTA_WIDTH,
    },
    responsive_quota_layout::{
        QuotaBodyLayoutMode, focused_row_index_for_account, quota_body_budget, quota_body_layout,
        quota_title_row, selected_detail_height, truncate_quota_header_text,
    },
};
use iocraft::prelude::*;

pub(crate) fn render_static_quota_status(
    props: &QuotaStatusComponentProps,
    mut hooks: Hooks,
) -> AnyElement<'static> {
    let model = &props.view_model;
    let width = props.width.max(MIN_QUOTA_WIDTH);
    let height = if props.height == 0 {
        MIN_RENDER_HEIGHT
    } else {
        props.height.max(1)
    };
    let content_width = width.saturating_sub(4);
    let focused = model
        .rows
        .iter()
        .find(|row| row.selected)
        .or_else(|| model.rows.first())
        .map(|row| row.account_id.clone());
    let floor = hooks.use_state(|| None);
    let focused_index = focused_row_index_for_account(&model.rows, focused.as_ref());
    let details = focused_index
        .and_then(|index| model.rows.get(index))
        .map(|row| &row.details);
    let detail_height = selected_detail_height(details.is_some(), QuotaInteractionMode::Static);
    let sidecar = width >= SIDECAR_QUOTA_WIDTH;
    let stacked =
        !sidecar && width >= NARROW_QUOTA_WIDTH && (details.is_some() || model.selected.is_none());
    let mode = if sidecar {
        QuotaBodyLayoutMode::Sidecar
    } else if stacked {
        QuotaBodyLayoutMode::Stacked {
            fill_available_height: true,
            prioritize_details: false,
        }
    } else {
        QuotaBodyLayoutMode::Inline
    };
    let layout = quota_body_layout(
        quota_body_budget(height),
        mode,
        model.rows.len(),
        focused_index,
        detail_height,
    );
    let body = super::quota_body_rendering::render_quota_body(
        super::quota_body_rendering::QuotaBodyViewProps {
            sidecar,
            show_stacked_details: stacked,
            filter_modal_owns_input: false,
            hide_list: false,
            body_height: layout.body_height,
            stacked_details_height: detail_height,
            list: super::quota_browse_rendering::QuotaAccountListProps {
                rows: &model.rows,
                compact_phone: false,
                width: content_width,
                height: layout.list_height,
                focused_row_index: focused_index,
                visible_rows: model.rows.len(),
                empty_notice: None,
            },
            panel: super::quota_detail_panel::QuotaDetailPanelProps {
                interactive: false,
                focused_details: details,
                account_options: None,
                reset_snapshot: None,
                reset_target: None,
                weekly_floor_editor: None,
                weekly_floor_editor_state: floor,
                width: content_width,
                height: detail_height,
                inventory_page_start: 0,
                spinner_tick: 0,
            },
        },
    );
    let footer = super::quota_help_rendering::quota_legacy_footer(
        super::quota_help_rendering::QuotaFooterPresentation {
            weekly_floor_editor: None,
            account_options: None,
            reset_snapshot: None,
            width: content_width,
            weekly_floor_editing_enabled: props.weekly_floor_saver.is_some(),
            reset_detail_active: false,
            filters: QuotaFilterState::default(),
            filter_modal_owns_input: false,
        },
    );
    super::quota_help_rendering::render_quota_screen(super::quota_help_rendering::QuotaScreenChromeProps {
        width,horizontal_padding:1,height:None,header:quota_title_row(model,content_width,0),
        route:element!(Text(content:truncate_quota_header_text(&model.route_line,content_width),color:Color::White,weight:Weight::Bold,wrap:TextWrap::NoWrap)).into_any(),
        controls:Vec::new(),body,actions:Vec::new(),static_footer:Some(super::quota_browse_rendering::fit_line(&footer,content_width)),
    })
}
