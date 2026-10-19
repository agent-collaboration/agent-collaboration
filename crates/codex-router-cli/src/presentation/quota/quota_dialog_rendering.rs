//! Quota content composed into the shared dialog; effects stay with the owners.
use super::quota_browse_rendering::render_selected_details;
use super::quota_detail_panel::*;
use super::quota_provider_filter_rendering::*;
use super::quota_provider_filters::*;
use super::quota_status_view_model::QuotaStatusViewModel;
use codex_router_core::ids::AccountId;
use codex_router_tui::{DialogAction, DialogFrame, DialogMargins, DialogPresentation};
use iocraft::prelude::*;

pub(super) struct QuotaDialogViewProps<'a> {
    pub width: u16,
    pub height: u16,
    pub filter_state: QuotaFilterState,
    pub filter_controller: QuotaFilterController,
    pub details_dialog: State<Option<AccountId>>,
    pub view_model: &'a QuotaStatusViewModel,
    pub filter_modal_owns_input: bool,
    pub help_action: Option<AnyElement<'static>>,
    pub actions: Vec<AnyElement<'static>>,
    pub account_options_state: State<Option<super::quota_account_options::AccountOptionsState>>,
    pub panel: QuotaDetailPanelProps<'a>,
}

/// The options panel and its page-key handler share the same visible capacity.
pub(super) fn quota_dialog_panel_height(viewport_height: usize, content_height: usize) -> usize {
    let height = u16::try_from(viewport_height).unwrap_or(u16::MAX);
    let (_, (_, frame_height)) = DialogPresentation::default().resolve(0, height.saturating_sub(2));
    let body_height = usize::from(frame_height.saturating_sub(6));
    content_height.min(body_height)
}

pub(super) fn render_quota_dialog(props: QuotaDialogViewProps<'_>) -> Option<AnyElement<'static>> {
    let QuotaDialogViewProps {
        width,
        height,
        filter_state,
        filter_controller,
        mut details_dialog,
        view_model,
        filter_modal_owns_input,
        help_action,
        actions,
        account_options_state,
        mut panel,
    } = props;
    let presentation = DialogPresentation::default();
    let margins = DialogMargins {
        top: 1,
        right: 2,
        bottom: 1,
        left: 2,
    };
    let ((_, frame_width), (_, frame_height)) =
        presentation.resolve(width.saturating_sub(4), height.saturating_sub(2));
    panel.width = usize::from(frame_width.saturating_sub(5));
    panel.height = usize::from(frame_height.saturating_sub(6));
    if let Some(draft) = filter_state.draft.filter(|_| !filter_modal_owns_input) {
        return Some(element! {
            DialogFrame(width,height,keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,title:"Account filters".to_owned(),help_action,
                presentation: DialogPresentation::default(),
                margins,
                actions:render_quota_filter_actions(draft,filter_controller)) {
                #(render_quota_filter_chooser(draft,panel.width,filter_controller))
            }
        }.into_any());
    }
    if details_dialog.read().is_none() && !filter_modal_owns_input {
        return None;
    }
    if let Some(identity) = details_dialog.read().as_ref() {
        panel.focused_details = view_model
            .rows
            .iter()
            .find(|row| &row.account_id == identity)
            .map(|row| &row.details);
    }
    let (title, content) = if let Some(options) = panel.account_options {
        (
            if options.editor.is_some() {
                "Credit usage"
            } else {
                match options.tab {
                    super::quota_account_options::AccountOptionsTab::Resets => {
                        "Account options · Resets"
                    }
                    super::quota_account_options::AccountOptionsTab::Credits => {
                        "Account options · Credits"
                    }
                }
            }
            .to_owned(),
            super::quota_account_options::render_account_options_content(
                super::quota_account_options::AccountOptionsPanelProps {
                    options,
                    reset_snapshot: panel.reset_snapshot,
                    reset_target: panel.reset_target,
                    width: panel.width,
                    height: panel.height,
                    inventory_page_start: panel.inventory_page_start,
                    spinner_tick: panel.spinner_tick,
                },
                Some(account_options_state),
            ),
        )
    } else if let Some(editor) = panel.weekly_floor_editor {
        (
            "Weekly floor".to_owned(),
            super::quota_floor_editor::render_weekly_floor_editor_content(
                editor,
                panel.width,
                panel.weekly_floor_editor_state,
            ),
        )
    } else if let (Some(snapshot), Some(target)) = (panel.reset_snapshot, panel.reset_target) {
        let title = match snapshot.phase() {
            crate::quota_reset::reset_session_supervisor::WorkflowPhase::Browse => "Reset credits",
            crate::quota_reset::reset_session_supervisor::WorkflowPhase::Inspecting => {
                "Inspecting resets"
            }
            crate::quota_reset::reset_session_supervisor::WorkflowPhase::Inspected => {
                "Reset inventory"
            }
            crate::quota_reset::reset_session_supervisor::WorkflowPhase::Confirming => {
                "Confirm reset"
            }
            crate::quota_reset::reset_session_supervisor::WorkflowPhase::Revalidating => {
                "Checking reset"
            }
            crate::quota_reset::reset_session_supervisor::WorkflowPhase::Committing => {
                "Reset in progress"
            }
            crate::quota_reset::reset_session_supervisor::WorkflowPhase::Result => "Reset result",
        };
        (
            title.to_owned(),
            super::quota_reset_detail_rendering::render_reset_panel_content(
                snapshot,
                target,
                panel.width,
                panel.height,
                panel.inventory_page_start,
                super::quota_reset_presentation_model::reset_inventory_page_size(panel.height),
                panel.spinner_tick,
            ),
        )
    } else {
        (
            "Account details".to_owned(),
            panel
                .focused_details
                .map(|details| render_selected_details(details, panel.width, true))
                .unwrap_or_else(|| {
                    element!(Text(content:"No selectable account",color:Color::Grey)).into_any()
                }),
        )
    };
    Some(element! {
        DialogFrame(width,height,keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,title,presentation,margins,help_action,
            scroll_keys:if panel.account_options.and_then(|options|options.editor.as_ref()).is_some_and(|editor|editor.choice.is_open()) {codex_router_tui::DialogScrollKeys::Disabled}else if filter_modal_owns_input {codex_router_tui::DialogScrollKeys::Arrows}else {codex_router_tui::DialogScrollKeys::Navigation},
            actions:if filter_modal_owns_input {
                actions
            } else {
                vec![element!(DialogAction(label:"Close",handler:move |_|details_dialog.set(None))).into_any()]
            }) {
            #(content)
        }
    }.into_any())
}
