//! Picker detail view.
use super::{
    START_NEW_DETAILS_HEIGHT, SessionConversationPreview, SessionPickerRecord, SessionsPickerModel,
    fit_line, no_matching_sessions_label, start_new_args_label,
};
use codex_router_tui::{UiHeading, UiHeadingLevel};
use iocraft::prelude::*;

pub(super) fn render_start_new_details(
    model: &SessionsPickerModel,
    width: usize,
    height: usize,
) -> AnyElement<'static> {
    let detail_width = width.saturating_sub(4);
    element! {
        View(
            width: width as u32,
            height: height.max(START_NEW_DETAILS_HEIGHT) as u32,
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: Color::DarkGrey,
            padding_left: 1,
            padding_right: 1,
            overflow: Overflow::Hidden,
        ) {
            UiHeading(content: "New Codex session".to_owned(), level: UiHeadingLevel::Section)
            #((!model.request.new_session_args_display.is_empty()).then(||element!(Text(content: fit_line(&start_new_args_label(model), detail_width), color: Color::White, wrap: TextWrap::NoWrap))))
            #(if model.visible_record_len() == 0 {
                Some(detail_text(no_matching_sessions_label(model), detail_width, Color::Grey))
            } else {
                None
            })
        }
    }
    .into_any()
}

pub(super) fn render_details(
    record: &SessionPickerRecord,
    width: usize,
    selected_conversation: Option<&SessionConversationPreview>,
    height: Size,
) -> AnyElement<'static> {
    if height == Size::Auto {
        return render_structured_details(record, width, selected_conversation);
    }
    // Passive sidecar keeps its existing bounded, single-line facts.
    let wrap = TextWrap::NoWrap;
    if record.identity.is_provider() {
        let detail_width = width.saturating_sub(4);
        let display_line = |value: String| fit_line(&value, detail_width);
        let endpoint = record.endpoint_label.as_deref().unwrap_or("Provider");
        let state = provider_state_label(record);
        return element! {
            View(
                width: width as u32,
                height,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0_f32,
                border_style: BorderStyle::Single,
                border_color: Color::DarkGrey,
                overflow: Overflow::Hidden,
                padding_left:1,
                padding_right:1,
            ) {
                Text(content:record.full_title.clone(),color:Color::White,wrap)
                Text(content:format!("Client: {}",record.client_provider_family().label()),color:Color::White,wrap)
                Text(content:display_line(format!("Machine: {}",record.machine_label())),color:Color::Grey,wrap)
                Text(content: display_line(format!("Session ID: {}", record.session_id)), color: Color::Grey, wrap)
                #(record.native_background.as_ref().map(|job|element!(Text(content:display_line(format!("Job handle: {}",String::from(job.job_handle.clone()))),color:Color::Grey,wrap))))
                #(record.native_background.as_ref().map(|job|element!(Text(content:display_line(format!("Job state: {}",native_job_state_label(&job.state))),color:Color::Grey,wrap))))
                #(record.native_peer.as_ref().map(|peer|element!(Text(content:display_line(format!("Peer status: {:?}",peer.status)),color:Color::Grey,wrap))))
                Text(content: display_line(format!("Endpoint: {endpoint}")), color: Color::Grey, wrap)
                Text(content: display_line(format!("State: {state}")), color: Color::Grey, wrap)
                Text(content: display_line(format!("Working directory: {}", record.cwd.as_deref().unwrap_or("-"))), color: Color::Grey, wrap)
            }
        }
        .into_any();
    }
    let conversation = selected_conversation.unwrap_or(&record.conversation);
    let conversation_rows = if conversation.snippets.is_empty() {
        vec![conversation_text(
            conversation
                .unavailable_reason
                .as_deref()
                .unwrap_or("history unavailable"),
            Color::DarkGrey,
        )]
    } else {
        conversation
            .snippets
            .iter()
            .map(|snippet| conversation_text(&format!("• {snippet}"), Color::Grey))
            .collect::<Vec<_>>()
    };

    element! {
        View(
            width: width as u32,
            height,
            flex_shrink: 0.0_f32,
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: Color::DarkGrey,
            overflow: Overflow::Hidden,
            padding_left:1,
            padding_right:1,
        ) {
            Text(content: format!("Session ID: {}", record.session_id), color: Color::Grey, weight: Weight::Normal, wrap)
            View(height: 1) { Text(content: "") }
            UiHeading(content: "Conversation".to_owned(), level: UiHeadingLevel::Section)
            #(conversation_rows)
        }
    }
    .into_any()
}

fn native_job_state_label(
    state: &collaboration_client::protocol::NativeClaudeBackgroundState,
) -> String {
    use collaboration_client::protocol::NativeClaudeBackgroundState;
    match state {
        NativeClaudeBackgroundState::Working => "Working".to_owned(),
        NativeClaudeBackgroundState::Blocked => "Blocked".to_owned(),
        NativeClaudeBackgroundState::Done => "Done".to_owned(),
        NativeClaudeBackgroundState::Failed => "Failed".to_owned(),
        NativeClaudeBackgroundState::Stopped => "Stopped".to_owned(),
        NativeClaudeBackgroundState::Unknown { state } => {
            format!("Unknown ({})", String::from(state.clone()))
        }
    }
}

pub(super) fn conversation_text(value: &str, color: Color) -> AnyElement<'static> {
    element! {
        Text(
            content: value.to_owned(),
            color,
            weight: Weight::Normal,
            wrap: TextWrap::Wrap,
        )
    }
    .into_any()
}

pub(super) fn detail_text(value: &str, width: usize, color: Color) -> AnyElement<'static> {
    element! {
        Text(
            content: fit_line(value, width),
            color,
            weight: Weight::Normal,
            wrap: TextWrap::NoWrap,
        )
    }
    .into_any()
}

/// The modal body retains original values; its fixed context may shorten a title.
fn render_structured_details(
    record: &SessionPickerRecord,
    width: usize,
    selected_conversation: Option<&SessionConversationPreview>,
) -> AnyElement<'static> {
    let field = |label: &str, value: String, secondary: bool| {
        element! {
        codex_router_tui::FormField(label:label.to_owned(),label_width:18u16,stacked:width<52) {
            codex_router_tui::UiText(content:value,role:if secondary {codex_router_tui::UiTextRole::Secondary}else {codex_router_tui::UiTextRole::Body},wrap:TextWrap::Wrap)
        }
    }.into_any()
    };
    let mut fields = vec![field("Session ID:", record.session_id.clone(), false)];
    if record.identity.is_provider() {
        fields.extend([
            field(
                "Endpoint:",
                record
                    .endpoint_label
                    .clone()
                    .unwrap_or_else(|| "Provider".to_owned()),
                true,
            ),
            field("State:", provider_state_label(record).to_owned(), true),
            field(
                "Client:",
                record.client_provider_family().label().to_owned(),
                false,
            ),
            field("Machine:", record.machine_label().to_owned(), true),
        ]);
        if let Some(job) = &record.native_background {
            fields.push(field(
                "Job handle:",
                String::from(job.job_handle.clone()),
                true,
            ));
            fields.push(field(
                "Job state:",
                native_job_state_label(&job.state),
                true,
            ));
        }
        if let Some(peer) = &record.native_peer {
            fields.push(field("Peer status:", format!("{:?}", peer.status), true));
        }
    }
    fields.push(field(
        "Working directory:",
        record.cwd.clone().unwrap_or_else(|| "-".to_owned()),
        false,
    ));
    let conversation = selected_conversation.unwrap_or(&record.conversation);
    let history = if conversation.snippets.is_empty() {
        vec![conversation_text(
            conversation
                .unavailable_reason
                .as_deref()
                .unwrap_or("history unavailable"),
            Color::DarkGrey,
        )]
    } else {
        conversation
            .snippets
            .iter()
            .map(|snippet| conversation_text(&format!("• {snippet}"), Color::Grey))
            .collect()
    };
    element! {
        View(width:width as u32,max_width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            #(fields)
            UiHeading(content:"Original title",level:UiHeadingLevel::Section)
            codex_router_tui::UiText(content:if record.full_title.is_empty() {record.title.clone()}else {record.full_title.clone()},role:codex_router_tui::UiTextRole::Body,wrap:TextWrap::Wrap)
            UiHeading(content:"Conversation",level:UiHeadingLevel::Section)
            #(history)
        }
    }.into_any()
}

fn provider_state_label(record: &SessionPickerRecord) -> &str {
    if matches!(
        &record.identity,
        crate::sessions::SessionPickerIdentity::NativeClaude(_)
    ) {
        return record.runtime_status.label();
    }
    match record.provider_state {
        Some(collaboration_client::protocol::ProviderSessionState::Unloaded) => "Unloaded",
        Some(collaboration_client::protocol::ProviderSessionState::Idle) => "Idle",
        Some(collaboration_client::protocol::ProviderSessionState::Running) => "Running",
        Some(collaboration_client::protocol::ProviderSessionState::RequiresAction) => {
            "Requires action"
        }
        Some(collaboration_client::protocol::ProviderSessionState::AuthenticationRequired) => {
            "Authentication required"
        }
        Some(collaboration_client::protocol::ProviderSessionState::Closed) => "Closed",
        None => "Unknown",
    }
}
