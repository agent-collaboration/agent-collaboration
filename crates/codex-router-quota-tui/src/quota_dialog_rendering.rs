//! Quota content composed into the shared dialog; effects stay with the owners.
use super::quota_browse_rendering::render_selected_details;
use super::quota_detail_panel::QuotaDetailPanelProps;
use super::quota_provider_filter_rendering::{
    render_quota_filter_actions, render_quota_filter_chooser,
};
use super::quota_provider_filters::{QuotaFilterController, QuotaFilterState};
use codex_router_core::ids::AccountId;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
use codex_router_tui::{DialogAction, DialogFrame, DialogMargins, DialogPresentation};
use iocraft::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuotaDetailsDialog {
    Account(AccountId),
    Pool,
}
impl QuotaDetailsDialog {
    pub(crate) fn account_id(&self) -> Option<&AccountId> {
        match self {
            Self::Account(id) => Some(id),
            Self::Pool => None,
        }
    }
}

pub(crate) struct QuotaDialogViewProps<'a> {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) identity: Option<&'a super::quota_dialog_context::QuotaDialogIdentity>,
    pub(crate) confirmation_choices: Option<AnyElement<'static>>,
    pub(crate) filter_state: QuotaFilterState,
    pub(crate) filter_controller: QuotaFilterController,
    pub(crate) details_dialog: State<Option<QuotaDetailsDialog>>,
    pub(crate) view_model: &'a QuotaStatusViewModel,
    pub(crate) filter_modal_owns_input: bool,
    pub(crate) help_action: Option<AnyElement<'static>>,
    pub(crate) actions: Vec<AnyElement<'static>>,
    pub(crate) account_options_state:
        State<Option<super::quota_account_options::AccountOptionsState>>,
    pub(crate) panel: QuotaDetailPanelProps<'a>,
}

/// The options panel and its page-key handler share the same visible capacity.
pub(crate) fn quota_dialog_panel_height(viewport_height: usize, content_height: usize) -> usize {
    let height = u16::try_from(viewport_height).unwrap_or(u16::MAX);
    let (_, (_, frame_height)) = DialogPresentation::default().resolve(0, height.saturating_sub(2));
    let body_height = usize::from(frame_height.saturating_sub(6));
    content_height.min(body_height)
}

pub(crate) fn render_quota_dialog(props: QuotaDialogViewProps<'_>) -> Option<AnyElement<'static>> {
    let QuotaDialogViewProps {
        width,
        height,
        identity,
        mut confirmation_choices,
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
    if matches!(
        details_dialog.read().as_ref(),
        Some(QuotaDetailsDialog::Pool)
    ) && !filter_modal_owns_input
    {
        return Some(super::quota_pool_details::render_pool_details(
            width,
            height,
            &view_model.header,
            view_model.selection_projection_degraded,
            details_dialog,
        ));
    }
    let presentation = if let Some(editor) = panel.weekly_floor_editor.filter(|_| height >= 18) {
        DialogPresentation::Sized {
            width: codex_router_tui::DialogAxisSize::Content { maximum: Some(68) },
            // Status adds one row and one separator to the editing form.
            height: codex_router_tui::DialogAxisSize::cells(
                if matches!(
                    editor.phase,
                    super::quota_floor_editor::WeeklyFloorEditorPhase::Editing
                ) {
                    14
                } else {
                    16
                },
            ),
        }
    } else {
        DialogPresentation::default()
    };
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
    if let Some(identity) = details_dialog
        .read()
        .as_ref()
        .and_then(QuotaDetailsDialog::account_id)
    {
        panel.focused_details = view_model
            .rows
            .iter()
            .find(|row| &row.account_id == identity)
            .map(|row| &row.details);
    }
    let (title, content) = if let (Some(snapshot), Some(target)) =
        (panel.reset_snapshot, panel.reset_target)
        && matches!(
            snapshot.phase(),
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Confirming
                | codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Revalidating
                | codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Committing
                | codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Result
        ) {
        let title = match snapshot.phase() {
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Confirming => {
                "Confirm reset"
            }
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Revalidating => {
                "Checking reset"
            }
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Committing => {
                "Reset in progress"
            }
            _ => "Reset result",
        };
        (
            title.to_owned(),
            super::quota_reset_detail_rendering::render_reset_panel_content(
                snapshot,
                target,
                panel.width,
                panel.inventory_page_start,
                super::quota_reset_presentation_model::reset_inventory_page_size(panel.height),
                panel.spinner_tick,
                confirmation_choices.take(),
            ),
        )
    } else if let Some(options) = panel.account_options {
        (
            if options.editor.is_some() {
                "Credit usage"
            } else {
                match options.tab {
                    super::quota_account_options::AccountOptionsTab::Resets => "Options · Resets",
                    super::quota_account_options::AccountOptionsTab::Credits => "Options · Credits",
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
                u16::from(height >= 18),
            ),
        )
    } else if let (Some(snapshot), Some(target)) = (panel.reset_snapshot, panel.reset_target) {
        let title = match snapshot.phase() {
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Browse => "Reset credits",
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Inspecting => {
                "Inspecting resets"
            }
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Inspected => {
                "Reset inventory"
            }
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Confirming => {
                "Confirm reset"
            }
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Revalidating => {
                "Checking reset"
            }
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Committing => {
                "Reset in progress"
            }
            codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Result => "Reset result",
        };
        (
            title.to_owned(),
            super::quota_reset_detail_rendering::render_reset_panel_content(
                snapshot,
                target,
                panel.width,
                panel.inventory_page_start,
                super::quota_reset_presentation_model::reset_inventory_page_size(panel.height),
                panel.spinner_tick,
                None,
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
            padding:panel.weekly_floor_editor.filter(|_| height >= 18).map(|_| 1u16),
            context:identity.map(|identity|vec![identity.render(usize::from(frame_width.saturating_sub(4)))]).unwrap_or_default(),
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
