//! Fork content and callbacks reuse shared chrome; the captured selection owns the effect.
use super::{
    SessionsPickerOutcome,
    picker_fork_confirmation::{ForkConfirmation, ForkDestinationKey, ForkLaunchTarget},
    picker_model::SessionsPickerModel,
};
use codex_router_tui::{
    DialogAction, DialogFrame, DialogKeyboardOwner, DialogMargins, DialogPresentation, FormField,
    SearchSelect,
};
use iocraft::prelude::*;

pub(super) fn confirm_fork(
    mut state: State<SessionsPickerModel>,
    mut outcome: State<Option<SessionsPickerOutcome>>,
) {
    let mut model = state.write();
    if model.show_help || model.client_action.is_active() || model.source_action.pending.is_some() {
        return;
    }
    if model
        .fork_confirmation
        .as_ref()
        .is_some_and(|confirmation| confirmation.search.is_open())
    {
        return;
    }
    if !model.begin_source_fork()
        && let Some(confirmation) = model.fork_confirmation.as_mut()
    {
        match confirmation.confirm() {
            Some(ForkLaunchTarget::Codex(selection)) => {
                outcome.set(Some(SessionsPickerOutcome::ForkSession(*selection)))
            }
            Some(ForkLaunchTarget::Native(selection)) => {
                model.begin_native_fork(*selection);
            }
            Some(ForkLaunchTarget::Unsupported) | None => {}
        }
    }
}

pub(super) fn render_fork_confirmation(
    confirmation: &ForkConfirmation,
    width: usize,
    height: usize,
    help_action: AnyElement<'static>,
    inactive: bool,
    mut state: State<SessionsPickerModel>,
    outcome: State<Option<SessionsPickerOutcome>>,
) -> Element<'static, View> {
    let width = u16::try_from(width).unwrap_or(u16::MAX);
    let height = u16::try_from(height).unwrap_or(u16::MAX);
    let choices = confirmation.choice_options();
    let expanded = confirmation.search.is_open();
    let eligible = confirmation
        .destinations
        .get(confirmation.focused_destination)
        .is_some_and(|destination| {
            destination.availability == super::picker_fork_confirmation::ForkAvailability::Eligible
        });
    let enabled = if expanded {
        confirmation
            .search
            .available_focused_key(&choices)
            .is_some()
    } else {
        !inactive && eligible
    };
    let ((_, frame_width), _) =
        DialogPresentation::default().resolve(width.saturating_sub(4), height.saturating_sub(2));
    // The longest field label, gap and searchable value fit in38 content cells.
    // Below that, use the shared stacked form instead of clipping either column.
    let stacked = frame_width.saturating_sub(4) < 38;
    let session_id = confirmation.session_id_label();
    let context = vec![super::session_dialog_context::render_session_context(
        &confirmation.title,
        session_id.as_deref(),
    )];
    let destination = confirmation
        .destinations
        .get(confirmation.focused_destination)
        .map(|destination| destination.label.clone())
        .unwrap_or_else(|| "Unavailable".to_owned());
    let field = if confirmation.search.is_open() {
        element!(SearchSelect<ForkDestinationKey>(state:confirmation.search.clone(),options:choices,on_select:move |key| {let mut state=state;if let Some(confirmation)=state.write().fork_confirmation.as_mut() {confirmation.select_destination(key);}})).into_any()
    } else {
        element! {codex_router_tui::DialogButton(handler:move |_|if let Some(confirmation)=state.write().fork_confirmation.as_mut() && let Some(destination)=confirmation.destinations.get(confirmation.focused_destination) {confirmation.search.open(destination.key.clone());}) {Text(content:format!("{destination} ▾"),color:Color::White)}}.into_any()
    };
    let explanation = confirmation.notice.or_else(|| {
        confirmation
            .destinations
            .get(confirmation.focused_destination)
            .and_then(|destination| destination.availability.explanation())
    });
    element! {
        View(width:u32::from(width),height:u32::from(height)) {
            DialogFrame(width,height,title:"Fork session".to_owned(),context,keyboard_owner:DialogKeyboardOwner::Caller,
                scroll_keys:if expanded {codex_router_tui::DialogScrollKeys::Disabled}else {codex_router_tui::DialogScrollKeys::Pages},
                margins:DialogMargins {top:1,right:2,bottom:1,left:2},help_action:Some(help_action),
                actions:vec![element!(DialogAction(label:"Cancel",keyboard_owner:DialogKeyboardOwner::Caller,handler:move |_|state.write().fork_confirmation=None)).into_any(),element!(DialogAction(label:if expanded {"Select"}else {"Fork"},enabled,keyboard_owner:DialogKeyboardOwner::Caller,has_focus:!expanded&&!inactive,handler:move |_| {
                    if expanded {
                        let selected={let current=state.read();current.fork_confirmation.as_ref().and_then(|confirmation|confirmation.search.available_focused_key(&confirmation.choice_options()))};
                        if let Some(key)=selected && let Some(confirmation)=state.write().fork_confirmation.as_mut() {confirmation.select_destination(key);}
                    }else {confirm_fork(state,outcome);}
                })).into_any()]) {
                FormField(label:"Source".to_owned(),label_width:11u16,stacked) {Text(content:confirmation.source_label.clone(),color:Color::Grey,wrap:TextWrap::Wrap)}
                FormField(label:"Directory".to_owned(),label_width:11u16,stacked) {Text(content:confirmation.directory_label.clone(),color:Color::White,wrap:TextWrap::Wrap)
                    Text(content:format!("Directory origin: {}",confirmation.directory_origin),color:Color::Grey)}
                FormField(label:"Destination".to_owned(),label_width:11u16,stacked,has_focus:true) {
                    #(if expanded {field}else {element!(codex_router_tui::SelectableRow(selected:false) {#(field)}).into_any()})
                }
                #(explanation.map(|message|element!(View(padding_left:if stacked {4}else {15}) {Text(content:message,color:Color::Grey)})))
            }
        }
    }
}
