//! Shared dialog chrome with session-owned, captured Details actions.
use super::super::details_dialog_state::{ClipboardRequestStatus, SessionDetailsTarget};
use super::super::session_id_clipboard::SessionClipboardWriter;
use super::{
    AnyElement, SessionConversationPreview, SessionsPickerModel, SessionsPickerOutcome, Size,
    State, View, element, render_details,
};
use codex_router_tui::{
    DialogAction, DialogFrame, DialogKeyboardOwner, DialogMargins, DialogPresentation, UiText,
    UiTextRole,
};

pub(super) fn render_details_dialog(
    model: &SessionsPickerModel,
    model_state: State<SessionsPickerModel>,
    selected_outcome: State<Option<SessionsPickerOutcome>>,
    selected_conversation: Option<&SessionConversationPreview>,
    width: u16,
    height: u16,
    copier: SessionClipboardWriter,
) -> AnyElement<'static> {
    let Some(dialog) = model.details_dialog.as_ref() else {
        return element!(View).into_any();
    };
    let title = "Session details".to_owned();
    let context = vec![
        super::super::session_dialog_context::render_session_context(
            &dialog.record().title,
            Some(&dialog.record().session_id),
        ),
    ];
    let expected_target = dialog.target.clone();
    let actions = dialog.actions().iter().map(|action| {
        let action = *action;
        let copier = copier.clone();
        let expected_target = expected_target.clone();
        element!(DialogAction(label: action.label(), has_focus: action == dialog.focused_action,
            keyboard_owner: DialogKeyboardOwner::Caller, handler: move |_| {
                let still_current = model_state.read().details_dialog.as_ref().is_some_and(|current| current.target == expected_target);
                if still_current {
                    super::super::details_dialog_actions::dispatch_details_action(model_state, selected_outcome, action, &copier);
                }
            })).into_any()
    }).collect::<Vec<_>>();
    let ((_, frame_width), _) =
        DialogPresentation::default().resolve(width.saturating_sub(4), height.saturating_sub(2));
    let content_width = usize::from(frame_width.saturating_sub(4));
    let content = match &dialog.target {
        SessionDetailsTarget::Session(record) => {
            render_details(record, content_width, selected_conversation, Size::Auto)
        }
    };
    let notice = match dialog.clipboard_status {
        ClipboardRequestStatus::Idle => None,
        ClipboardRequestStatus::Sent => Some(("Copy requested", UiTextRole::Secondary)),
        ClipboardRequestStatus::Failed => Some(("Could not copy", UiTextRole::Error)),
    };
    element! {
        DialogFrame(width, height, title, context, help_action: Some(super::super::picker_help_view::help_action(model_state)),
            presentation: DialogPresentation::default(),
            margins: DialogMargins { top: 1, right: 2, bottom: 1, left: 2 }, scroll_keys: codex_router_tui::DialogScrollKeys::Navigation, actions) {
            #(notice.map(|(content, role)| element!(UiText(content, role))))
            #(dialog.inspection_notice().map(|notice|element!(UiText(content:notice.to_owned(),role:UiTextRole::Secondary))))
            #(content)
            #(super::super::source_action_view::source_error(model))
        }
    }.into_any()
}
