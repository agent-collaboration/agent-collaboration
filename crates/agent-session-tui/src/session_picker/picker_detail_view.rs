//! Picker detail view.
use super::{SessionConversationPreview, SessionPickerRecord, fit_line};
use crate::session_picker::picker_rendering::iocraft_cell_count;
use agent_collaboration_tui::{
    UiHeading, UiHeadingLevel,
    foundation::theme::{TextRole, Theme},
    surfaces::surface_handle::SurfaceContext,
};
use iocraft::prelude::*;

pub(super) fn render_details(
    record: &SessionPickerRecord,
    width: usize,
    selected_conversation: Option<&SessionConversationPreview>,
    height: Size,
) -> AnyElement<'static> {
    element!(SessionDetailsBody(record:record.clone(),width,conversation:selected_conversation.cloned(),height)).into_any()
}

#[derive(Props)]
struct SessionDetailsBodyProps {
    #[iocraft(required)]
    record: SessionPickerRecord,
    width: usize,
    conversation: Option<SessionConversationPreview>,
    height: Size,
}
#[component]
fn SessionDetailsBody(
    props: &SessionDetailsBodyProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks
        .try_use_context::<SurfaceContext>()
        .map_or(Theme::default(), |surface| surface.theme());
    render_details_content(
        &props.record,
        props.width,
        props.conversation.as_ref(),
        props.height,
        theme,
    )
}

fn render_details_content(
    record: &SessionPickerRecord,
    width: usize,
    selected_conversation: Option<&SessionConversationPreview>,
    height: Size,
    theme: Theme,
) -> AnyElement<'static> {
    if height == Size::Auto {
        return render_structured_details(record, width, selected_conversation, theme);
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
                width: iocraft_cell_count(width),
                height,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0_f32,
                border_style: BorderStyle::Single,
                border_color: theme.foreground(TextRole::Dimmed),
                overflow: Overflow::Hidden,
                padding_left:1,
                padding_right:1,
            ) {
                Text(content:record.full_title.clone(),color:theme.foreground(TextRole::Text),wrap)
                Text(content:format!("Client: {}",record.client_provider_family().label()),color:theme.foreground(TextRole::Text),wrap)
                Text(content:display_line(format!("Machine: {}",record.machine_label())),color:theme.foreground(TextRole::Muted),wrap)
                Text(content: display_line(format!("Session ID: {}", record.session_id)), color: theme.foreground(TextRole::Muted), wrap)
                #(record.native_background.as_ref().map(|job|element!(Text(content:display_line(format!("Job handle: {}",String::from(job.job_handle.clone()))),color:theme.foreground(TextRole::Muted),wrap))))
                #(record.native_background.as_ref().map(|job|element!(Text(content:display_line(format!("Job state: {}",native_job_state_label(&job.state))),color:theme.foreground(TextRole::Muted),wrap))))
                #(record.native_background.as_ref().map(|job|element!(Text(content:display_line(format!("Job directory: {}",String::from(job.working_directory.clone()))),color:theme.foreground(TextRole::Muted),wrap))))
                #(record.native_conflict.as_ref().map(|reason|element!(Text(content:display_line(format!("Conflict: {}",reason.description())),color:theme.foreground(TextRole::Warning),wrap))))
                #(record.native_peer.as_ref().map(|peer|element!(Text(content:display_line(format!("Peer status: {:?}",peer.status)),color:theme.foreground(TextRole::Muted),wrap))))
                Text(content: display_line(format!("Endpoint: {endpoint}")), color: theme.foreground(TextRole::Muted), wrap)
                Text(content: display_line(format!("State: {state}")), color: theme.foreground(TextRole::Muted), wrap)
                Text(content: display_line(format!("Working directory: {}", record.cwd.as_deref().unwrap_or("-"))), color: theme.foreground(TextRole::Muted), wrap)
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
            theme.foreground(TextRole::Dimmed),
        )]
    } else {
        conversation
            .snippets
            .iter()
            .map(|snippet| {
                conversation_text(&format!("• {snippet}"), theme.foreground(TextRole::Muted))
            })
            .collect::<Vec<_>>()
    };

    element! {
        View(
            width: iocraft_cell_count(width),
            height,
            flex_shrink: 0.0_f32,
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: theme.foreground(TextRole::Dimmed),
            overflow: Overflow::Hidden,
            padding_left:1,
            padding_right:1,
        ) {
            Text(content: format!("Session ID: {}", record.session_id), color: theme.foreground(TextRole::Muted), weight: Weight::Normal, wrap)
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

/// The modal body retains original values; its fixed context may shorten a title.
fn render_structured_details(
    record: &SessionPickerRecord,
    width: usize,
    selected_conversation: Option<&SessionConversationPreview>,
    theme: Theme,
) -> AnyElement<'static> {
    let field = |label: &str, value: String, secondary: bool| {
        element! {
            agent_collaboration_tui::FormField(label:label.to_owned()) {
                agent_collaboration_tui::UiText(content:value,role:if secondary {agent_collaboration_tui::UiTextRole::Secondary}else {agent_collaboration_tui::UiTextRole::Body},wrap:TextWrap::Wrap)
            }
        }
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
            fields.push(field(
                "Job directory:",
                String::from(job.working_directory.clone()),
                false,
            ));
        }
        if let Some(reason) = &record.native_conflict {
            fields.push(field("Conflict:", reason.description().to_owned(), false));
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
            theme.foreground(TextRole::Dimmed),
        )]
    } else {
        conversation
            .snippets
            .iter()
            .map(|snippet| {
                conversation_text(&format!("• {snippet}"), theme.foreground(TextRole::Muted))
            })
            .collect()
    };
    element! {
        View(width:u32::try_from(width).unwrap_or(u32::MAX),max_width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            agent_collaboration_tui::dialog::DialogSections {
                agent_collaboration_tui::dialog::DialogSection {
                    agent_collaboration_tui::FormFields(stacked:width<52) {#(fields)}
                }
                agent_collaboration_tui::dialog::DialogSection(heading:"Original title".to_owned()) {
                    agent_collaboration_tui::UiText(content:if record.full_title.is_empty() {record.title.clone()}else {record.full_title.clone()},role:agent_collaboration_tui::UiTextRole::Body,wrap:TextWrap::Wrap)
                }
                agent_collaboration_tui::dialog::DialogSection(heading:"Conversation".to_owned()) {
                    #(history)
                }
            }
        }
    }.into_any()
}

fn provider_state_label(record: &SessionPickerRecord) -> &str {
    if matches!(
        &record.identity,
        agent_session_inventory::sessions::SessionPickerIdentity::NativeClaude(_)
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
