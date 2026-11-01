//! Printed quota chrome and notices; interactive actions are declared in control scopes.
use agent_collaboration_tui::{UiText, UiTextRole};
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
use iocraft::prelude::*;

pub(crate) fn render_quota_route(
    model: &QuotaStatusViewModel,
    width: usize,
) -> AnyElement<'static> {
    let _ = width;
    element! {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
        #(model.selection_projection_degraded.then(||element!(UiText(content:"Selection projection degraded",role:UiTextRole::Error,wrap:TextWrap::Wrap))))
        #(model.header.credential_notice.as_ref().map(|notice|element!(UiText(content:notice.clone(),role:UiTextRole::Error,wrap:TextWrap::Wrap))))
    }}.into_any()
}

pub(crate) struct QuotaScreenChromeProps {
    pub(crate) width: usize,
    pub(crate) horizontal_padding: usize,
    pub(crate) height: Option<usize>,
    pub(crate) header: AnyElement<'static>,
    pub(crate) route: AnyElement<'static>,
    pub(crate) controls: Vec<AnyElement<'static>>,
    pub(crate) body: AnyElement<'static>,
    pub(crate) actions: Vec<AnyElement<'static>>,
    pub(crate) static_footer: Option<String>,
}

pub(crate) fn render_quota_screen(props: QuotaScreenChromeProps) -> AnyElement<'static> {
    element! {
        View(width: u32::try_from(props.width).unwrap_or(u32::MAX), height: props.height.map_or(Size::Auto,|height|Size::Length(u32::try_from(height).unwrap_or(u32::MAX))), border_style: BorderStyle::Round, border_color: Color::Cyan, overflow: Overflow::Hidden, padding_left: u32::try_from(props.horizontal_padding).unwrap_or(u32::MAX), padding_right: u32::try_from(props.horizontal_padding).unwrap_or(u32::MAX), flex_direction: FlexDirection::Column) {
            #(props.header)
            #(props.route)
            #(props.controls)
            #(props.body)
            View(width: 100pct, flex_grow: 1.0_f32) {}
            #((!props.actions.is_empty()).then(||element!(agent_collaboration_tui::dialog::dialog_footer::DialogFooter(children:props.actions))))
            #(props.static_footer.map(|line| element!(Text(content: line, color: Color::Grey, wrap: TextWrap::NoWrap)).into_any()))
        }
    }.into_any()
}

// Legacy static output keeps its current workflow footer; interactive Help owns
// shortcut presentation instead. This function has no input or effect handling.
pub(crate) struct QuotaFooterPresentation<'a> {
    pub(crate) weekly_floor_editor: Option<&'a super::quota_floor_editor::WeeklyFloorEditorState>,
    pub(crate) account_options: Option<&'a super::quota_account_options::AccountOptionsState>,
    pub(crate) reset_snapshot:
        Option<&'a codex_router_quota_service::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot>,
    pub(crate) width: usize,
    pub(crate) weekly_floor_editing_enabled: bool,
    pub(crate) reset_detail_active: bool,
    pub(crate) filters: super::quota_provider_filters::QuotaFilterState,
    pub(crate) filter_modal_owns_input: bool,
}

pub(crate) fn quota_legacy_footer(props: QuotaFooterPresentation<'_>) -> String {
    use super::quota_account_options::account_options_footer;
    use super::quota_floor_editor::weekly_floor_editor_footer;
    use super::quota_reset_detail_rendering::reset_footer;
    let inspection_footer = props
        .account_options
        .and(props.reset_snapshot)
        .and_then(|snapshot| {
            if props
                .account_options
                .is_some_and(|options| options.pending_tab.is_some())
                && matches!(
                    snapshot.phase(),
                    codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Inspecting
                        | codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Inspected
                )
            {
                Some("tab change pending  esc back  ctrl-c exit")
            } else {
                super::quota_account_options::account_options_inspection_footer(
                    snapshot.phase(),
                    props.width,
                )
            }
        });
    let mut footer_line = props.weekly_floor_editor.map_or_else(
        || {
            props.account_options.map_or_else(
                || {
                    if props.weekly_floor_editing_enabled && !props.reset_detail_active {
                        "↑/↓ focus  ctrl-e edit floor  ctrl-r account options  esc/q exit  ctrl-c exit".to_owned()
                    } else {
                        reset_footer(props.reset_snapshot).to_owned()
                    }
                },
                |options| {
                    if props.reset_detail_active {
                        inspection_footer.map_or_else(
                            || reset_footer(props.reset_snapshot).to_owned(),
                            str::to_owned,
                        )
                    } else {
                        account_options_footer(options)
                    }
                },
            )
        },
        |editor| weekly_floor_editor_footer(editor).to_owned(),
    );
    if props.filters.draft.is_some() && !props.filter_modal_owns_input {
        footer_line = "↑/↓ provider  tab focus  space toggle  enter apply  esc cancel".to_owned();
    }
    footer_line
}
