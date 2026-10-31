//! Hermetic terminal-lifecycle observation; the external client ports are stand-ins.
use agent_session_inventory::sessions::{
    source_client_action::SessionClientEffect,
    source_ssh_protocol::{SourceNativeLaunchAvailability, SourceReadError},
};
use agent_session_tui::session_picker::{
    SessionClientPorts, SessionsPickerOutcome, SessionsPickerRequest,
    new_session_client_inventory::qualify_capabilities, run_sessions_picker,
};
use std::{
    io::{self, Write},
    process::{Command, Stdio},
    sync::Arc,
};

fn read_termios() -> io::Result<String> {
    let output = Command::new("/bin/stty")
        .arg("-g")
        .stdin(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("could not observe fixture terminal"));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(io::Error::other)
}

pub(crate) fn run() -> io::Result<()> {
    println!("CLAUDE_TERMIOS_BEFORE {}", read_termios()?);
    io::stdout().flush()?;
    let ports = SessionClientPorts {
        inventory_loader: Arc::new(|_| {
            Box::pin(async {
                let service = "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()
                    .map_err(|_| SourceReadError::InvalidReply)?;
                let inventory = serde_json::from_value(serde_json::json!({
                "serviceEpoch":"00000000-0000-4000-8000-000000000002", "sequence":0, "endpoints":[{
                    "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
                    "label":"Native Claude","channels":[],
                    "availability":{"state":"unavailable","observedAt":"2026-10-29T00:00:00Z","reason":"managed channel absent"}
                }]
            })).map_err(|_| SourceReadError::InvalidReply)?;
                qualify_capabilities(
                    &service,
                    inventory,
                    SourceNativeLaunchAvailability::Ready {
                        capabilities: vec![],
                    },
                )
            })
        }),
        executor: Arc::new(|_| {
            Box::pin(async {
                println!("CLAUDE_ASYNC_NATIVE_EXECUTION");
                let _ = io::stdout().flush();
                Err(SourceReadError::InvalidReply)
            })
        }),
        details_inspector: Arc::new(|_| Box::pin(async { Err(SourceReadError::InvalidReply) })),
    };
    let request = SessionsPickerRequest {
        current_dir: std::env::current_dir()?,
        native_arguments: ["--model", "haiku"].map(std::ffi::OsString::from).to_vec(),
        ..SessionsPickerRequest::default()
    };
    let outcome = run_sessions_picker(request, None, Some(ports), crossterm::terminal::size()?)?;
    let Some(SessionsPickerOutcome::LaunchNativeClaude(request, mode)) = outcome else {
        return Err(io::Error::other(
            "foreground Claude did not return its terminal handoff",
        ));
    };
    if !matches!(request.effect, SessionClientEffect::Native { .. }) {
        return Err(io::Error::other(
            "terminal handoff changed the captured effect",
        ));
    }
    println!("CLAUDE_TERMIOS_AFTER {}", read_termios()?);
    println!("CLAUDE_TERMINAL_MODE {mode:?}");
    println!("CLAUDE_TERMINAL_HANDOFF");
    if let Some(executable) = std::env::var_os("CLAUDE_TERMINAL_FIXTURE_EXECUTABLE") {
        use agent_session_inventory::sessions::{
            claude_terminal_launch::{prepare_claude_terminal_mode, run_claude_terminal},
            source_ssh_protocol::SourceNativeClaudeAction,
        };
        let SessionClientEffect::Native {
            action:
                SourceNativeClaudeAction::New {
                    directory,
                    arguments,
                },
            ..
        } = request.effect
        else {
            return Err(io::Error::other("lifecycle fixture requires New"));
        };
        let marker = std::env::var_os("CLAUDE_FIXTURE_TEARDOWN_MARKER")
            .ok_or_else(|| io::Error::other("missing teardown marker"))?;
        std::fs::write(marker, b"picker returned; terminal restored\n")?;
        let mut command = Command::new(executable);
        command.args(arguments).current_dir(directory.as_str());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let command = runtime
            .block_on(prepare_claude_terminal_mode(command, mode))
            .map_err(io::Error::other)?;
        drop(runtime);
        run_claude_terminal(command).map_err(io::Error::other)?;
        println!("CLAUDE_TERMIOS_RETURNED {}", read_termios()?);
    }
    Ok(())
}
