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
    initial_size: (u16, u16),
) -> io::Result<Option<SessionsPickerOutcome>> {
    render_picker(
        request,
        record_loader,
        client_ports,
        initial_size,
        PickerRenderSupport::default(),
    )
}

#[derive(Default)]
struct PickerRenderSupport {
    #[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
    dispatch_receipt:
        Option<agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink>,
    source_preparer: Option<super::super::source_action_worker::SourceActionPreparer>,
}

fn render_picker(
    request: SessionsPickerRequest,
    record_loader: Option<SessionsPickerRecordLoader>,
    client_ports: Option<super::super::SessionClientPorts>,
    initial_size: (u16, u16),
    support: PickerRenderSupport,
) -> io::Result<Option<SessionsPickerOutcome>> {
    let mut selected_outcome = None;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut component = element! {
        SessionsPickerComponent(
            request: request,
            record_loader: record_loader,
            source_preparer: support.source_preparer,
            client_inventory_loader: client_ports.as_ref().map(|ports|ports.inventory_loader.clone()),
            client_executor: client_ports.as_ref().map(|ports|ports.executor.clone()),
            details_client_inspector:client_ports.as_ref().map(|ports|ports.details_inspector.clone()),
            session_clipboard_writer: Some(super::super::session_id_clipboard::terminal_session_clipboard_writer()),
            width: usize::from(initial_size.0),
            height: usize::from(initial_size.1),
            selected_outcome_out: &mut selected_outcome,
        )
    };
    #[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
    {
        component.props.dispatch_receipt = support.dispatch_receipt.clone();
    }
    let render_loop = component
        .render_loop()
        .enable_bracketed_paste()
        .fullscreen()
        .ignore_ctrl_c();
    #[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
    let render_loop = match support.dispatch_receipt {
        Some(sink) => render_loop.stdout(sink.frame_writer(io::stdout())?),
        None => render_loop,
    };
    runtime.block_on(render_loop)?;
    drop(component);
    Ok(selected_outcome)
}

#[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
#[path = "picker_fixture_receipts.rs"]
mod picker_fixture_receipts;

#[cfg(feature = "quota-reset-test-harness")]
pub fn run_sessions_picker_test_harness() -> io::Result<()> {
    use std::io::Write;

    use super::super::provider_filter_storyboard_fixture::{
        apply_storyboard_variant, provider_filter_storyboard_request,
    };
    let variant = match std::env::var("CODEX_ROUTER_TUI_STORYBOARD_VARIANT") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
    };
    let request =
        apply_storyboard_variant(provider_filter_storyboard_request()?, variant.as_deref())?;

    let initial_size = crossterm::terminal::size()?;
    #[cfg(not(debug_assertions))]
    let support = PickerRenderSupport::default();
    #[cfg(debug_assertions)]
    let receipt_directory = picker_fixture_receipts::receipt_directory()?;
    #[cfg(debug_assertions)]
    let dispatch_receipt = receipt_directory
        .as_ref()
        .map(|directory| {
            agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink::create(
                directory.join("input-dispatch.jsonl"),
            )
        })
        .transpose()?;
    #[cfg(debug_assertions)]
    let support = PickerRenderSupport {
        dispatch_receipt: dispatch_receipt.clone(),
        source_preparer: Some(picker_fixture_receipts::recording_preparer(
            receipt_directory.as_deref(),
        )),
    };
    #[cfg(debug_assertions)]
    let scenario = variant
        .as_deref()
        .and_then(super::super::picker_fixture_scenarios::PickerFixtureScenario::from_name);
    #[cfg(debug_assertions)]
    let record_loader = super::super::picker_fixture_scenarios::source_updates::record_loader(
        scenario,
        receipt_directory.as_deref(),
    )?;
    #[cfg(debug_assertions)]
    let client_ports = super::super::picker_fixture_scenarios::client_ports(scenario, &request);
    #[cfg(not(debug_assertions))]
    let (record_loader, client_ports) = (None, None);
    #[cfg(debug_assertions)]
    let captured_request = request.clone();
    let outcome = render_picker(request, record_loader, client_ports, initial_size, support)?;
    #[cfg(debug_assertions)]
    {
        if let Some(sink) = dispatch_receipt {
            sink.flush_and_check()?;
        }
        if let Some(directory) = receipt_directory {
            match picker_fixture_receipts::write_outcome(&directory, &outcome, &captured_request) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                    eprintln!("Fixture outcome readback: {error}")
                }
                Err(error) => return Err(error),
            }
        }
    }
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
        Some(SessionsPickerOutcome::LaunchNativeClaude(_, _)) => {
            "SESSION_PICKER_OUTCOME native-claude".to_owned()
        }
        None => "SESSION_PICKER_OUTCOME canceled".to_owned(),
    };
    writeln!(io::stdout(), "{marker}")
}
