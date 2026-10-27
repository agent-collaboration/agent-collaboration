//! Terminal lifecycle and fixture entry points for the actual picker component.
use super::{
    ElementExt, SessionsPickerComponent, SessionsPickerOutcome, SessionsPickerRecordLoader,
    SessionsPickerRequest, element,
};
use std::io;

pub fn run_sessions_picker(
    request: SessionsPickerRequest,
    record_loader: Option<SessionsPickerRecordLoader>,
    client_ports: Option<super::super::SessionClientPorts>,
) -> io::Result<Option<SessionsPickerOutcome>> {
    let mut selected_outcome = None;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(
        element! {
            SessionsPickerComponent(
                request: request,
                record_loader: record_loader,
                client_inventory_loader: client_ports.as_ref().map(|ports|ports.inventory_loader.clone()),
                client_executor: client_ports.as_ref().map(|ports|ports.executor.clone()),
                details_client_inspector:client_ports.as_ref().map(|ports|ports.details_inspector.clone()),
                session_clipboard_writer: Some(super::super::session_id_clipboard::terminal_session_clipboard_writer()),
                width: 0usize,
                selected_outcome_out: &mut selected_outcome,
            )
        }
        .render_loop()
        .enable_bracketed_paste()
        .fullscreen()
        .ignore_ctrl_c(),
    )?;
    Ok(selected_outcome)
}

#[cfg(feature = "quota-reset-test-harness")]
pub fn run_sessions_picker_test_harness() -> io::Result<()> {
    use std::io::Write;

    let request =
        super::super::provider_filter_storyboard_fixture::provider_filter_storyboard_request()?;

    let outcome = run_sessions_picker(request, None, None)?;
    let marker = match outcome {
        Some(SessionsPickerOutcome::ResumeSession(selection)) => {
            format!("SESSION_PICKER_OUTCOME resume:{}", selection.session_id())
        }
        Some(SessionsPickerOutcome::ForkSession(selection)) => {
            format!("SESSION_PICKER_OUTCOME fork:{}", selection.session_id())
        }
        Some(SessionsPickerOutcome::StartNewSession) => {
            "SESSION_PICKER_OUTCOME start-new".to_owned()
        }
        Some(SessionsPickerOutcome::PreparedSourceSession(_)) => {
            "SESSION_PICKER_OUTCOME prepared-source".to_owned()
        }
        Some(SessionsPickerOutcome::TerminalTooNarrow) => {
            "SESSION_PICKER_OUTCOME terminal-too-narrow".to_owned()
        }
        Some(SessionsPickerOutcome::InterruptedClientAction(_)) => {
            "SESSION_PICKER_OUTCOME interrupted-client".to_owned()
        }
        None => "SESSION_PICKER_OUTCOME canceled".to_owned(),
    };
    writeln!(io::stdout(), "{marker}")
}
