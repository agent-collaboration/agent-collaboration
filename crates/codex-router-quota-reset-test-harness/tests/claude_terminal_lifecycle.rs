//! Actual iocraft PTY teardown; client inventory/effects are explicit stand-ins.
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use regex::Regex;
use std::{
    io::{self, Read, Write},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
const HANDOFF: &[u8] = b"CLAUDE_TERMINAL_HANDOFF";

struct OwnedChild(Box<dyn Child + Send + Sync>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct KeyboardEmulator {
    stack: Vec<u32>,
    sequence: Vec<u8>,
    transcript: Vec<u8>,
    flags_at_handoff: Option<Vec<u32>>,
    operations: Regex,
}

impl KeyboardEmulator {
    fn new() -> Result<Self, regex::Error> {
        Ok(Self {
            stack: vec![0, 5],
            sequence: Vec::new(),
            transcript: Vec::new(),
            flags_at_handoff: None,
            operations: Regex::new(r"^\x1b\[([><=])(\d*)(?:;(\d+))?u$")?,
        })
    }

    fn receive(&mut self, bytes: &[u8], writer: &mut dyn Write) -> io::Result<()> {
        for byte in bytes {
            self.transcript.push(*byte);
            if self.transcript.ends_with(HANDOFF) {
                self.flags_at_handoff = Some(self.stack.clone());
            }
            if self.sequence.is_empty() {
                if *byte == 0x1b {
                    self.sequence.push(*byte);
                }
                continue;
            }
            if self.sequence.len() == 1 {
                if *byte == b'[' {
                    self.sequence.push(*byte);
                } else {
                    self.sequence.clear();
                }
                continue;
            }
            self.sequence.push(*byte);
            if !(0x40..=0x7e).contains(byte) {
                continue;
            }
            let sequence =
                String::from_utf8(std::mem::take(&mut self.sequence)).map_err(io::Error::other)?;
            match sequence.as_str() {
                "\x1b[?u" => write!(
                    writer,
                    "\x1b[?{}u",
                    self.stack
                        .last()
                        .ok_or_else(|| io::Error::other("empty keyboard flag stack"))?
                )?,
                "\x1b[c" | "\x1b[0c" => writer.write_all(b"\x1b[?1;2c")?,
                "\x1b[6n" => writer.write_all(b"\x1b[1;1R")?,
                _ => {
                    if let Some(operation) = self.operations.captures(&sequence) {
                        let operand = operation[2].parse::<u32>().unwrap_or(1);
                        match &operation[1] {
                            ">" => self.stack.push(operand),
                            "<" => {
                                for _ in 0..operand {
                                    if self.stack.len() > 1 {
                                        self.stack.pop();
                                    }
                                }
                            }
                            "=" => {
                                let current = self
                                    .stack
                                    .last_mut()
                                    .ok_or_else(|| io::Error::other("empty keyboard flag stack"))?;
                                *current = match operation.get(3).map(|mode| mode.as_str()) {
                                    Some("2") => *current | operand,
                                    Some("3") => *current & !operand,
                                    _ => operand,
                                };
                            }
                            _ => {
                                return Err(io::Error::other(
                                    "unrecognized keyboard flag operation",
                                ));
                            }
                        }
                    }
                }
            }
            writer.flush()?;
        }
        Ok(())
    }
}

fn wait_for(
    receiver: &mpsc::Receiver<Vec<u8>>,
    transcript: &mut Vec<u8>,
    start: usize,
    expected: &str,
) -> TestResult<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let text = String::from_utf8_lossy(
            transcript
                .get(start..)
                .ok_or("invalid transcript checkpoint")?,
        );
        if text.contains("CLAUDE_ASYNC_NATIVE_EXECUTION") {
            return Err("native foreground action ran inside the mounted picker".into());
        }
        if text.contains(expected) {
            return Ok(());
        }
        let bytes = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| {
                format!(
                    "missing lifecycle event {expected}: {error}; {}",
                    String::from_utf8_lossy(transcript)
                )
            })?;
        transcript.extend(bytes);
    }
}

#[test]
fn foreground_claude_handoff_restores_termios_and_the_keyboard_flag_stack() -> TestResult<()> {
    run_claude_terminal_lifecycle("Claude Code", "Foreground")
}

#[test]
fn background_claude_starts_and_attaches_after_picker_teardown() -> TestResult<()> {
    run_claude_terminal_lifecycle("Claude Code · background", "BackgroundThenAttach")
}

struct LifecycleDirectory(std::path::PathBuf);

impl Drop for LifecycleDirectory {
    fn drop(&mut self) {
        let _cleanup = std::fs::remove_dir_all(&self.0);
    }
}

fn run_claude_terminal_lifecycle(client_label: &str, expected_mode: &str) -> TestResult<()> {
    let directory = LifecycleDirectory(std::env::temp_dir().join(format!(
        "claude-terminal-lifecycle-{}-{expected_mode}",
        std::process::id()
    )));
    std::fs::create_dir(&directory.0)?;
    let node = std::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()?;
    if !node.status.success() {
        return Err("Node fixture executable unavailable".into());
    }
    let node = String::from_utf8(node.stdout)?;
    let executable = directory.0.join("claude.mjs");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../verification/fixtures/native-claude-background-agent.ts")
        .canonicalize()?;
    std::fs::write(
        &executable,
        format!(
            "#!{}\nimport {{runFixture}} from {:?};\nrunFixture(null);\n",
            node.trim(),
            fixture
        ),
    )?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    let flow = directory.0.join("flow.jsonl");
    let signing = std::process::Command::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/cargo_debug_signing_runner.sh"),
    )
    .args([
        "--sign-only",
        env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness"),
    ])
    .status()?;
    if !signing.success() {
        return Err("sign the directly spawned harness before running it".into());
    }
    let pair = native_pty_system().openpty(PtySize {
        rows: 38,
        cols: 132,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command =
        CommandBuilder::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness"));
    command.args(["--sessions-picker", "--claude-terminal-lifecycle"]);
    command.env_clear();
    command.env("TERM", "xterm-256color");
    command.env("HOME", std::env::temp_dir());
    command.env("PATH", "/usr/bin:/bin");
    command.env("LANG", "C.UTF-8");
    command.env("CLAUDE_TERMINAL_FIXTURE_EXECUTABLE", &executable);
    command.env(
        "CLAUDE_FIXTURE_TEARDOWN_MARKER",
        directory.0.join("picker-returned"),
    );
    command.env("CLAUDE_FIXTURE_FLOW", &flow);
    command.env("CLAUDE_FIXTURE_COUNT", directory.0.join("count.txt"));
    command.env("CLAUDE_FIXTURE_RECORD", directory.0.join("record.json"));
    command.env(
        "CLAUDE_FIXTURE_RECEIPT",
        include_str!("../../agent-collaboration/tests/claude_background_cli/claude_bg_2_1_312.txt"),
    );
    command.cwd(env!("CARGO_MANIFEST_DIR"));
    let mut child = OwnedChild(pair.slave.spawn_command(command)?);
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
    let reader_writer = writer.clone();
    let (sender, receiver) = mpsc::channel();
    let reading = thread::spawn(move || -> io::Result<KeyboardEmulator> {
        let mut terminal = KeyboardEmulator::new().map_err(io::Error::other)?;
        let mut buffer = [0_u8; 8192];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.raw_os_error() == Some(5) => break,
                Err(error) => return Err(error),
            };
            let bytes = buffer
                .get(..count)
                .ok_or_else(|| io::Error::other("invalid PTY read count"))?;
            terminal.receive(
                bytes,
                reader_writer
                    .lock()
                    .map_err(|_| io::Error::other("terminal writer poisoned"))?
                    .as_mut(),
            )?;
            if sender.send(bytes.to_vec()).is_err() {
                break;
            }
        }
        Ok(terminal)
    });
    let send = |bytes: &[u8]| -> io::Result<()> {
        let mut writer = writer
            .lock()
            .map_err(|_| io::Error::other("terminal writer poisoned"))?;
        writer.write_all(bytes)?;
        writer.flush()
    };
    let mut transcript = Vec::new();
    wait_for(&receiver, &mut transcript, 0, "Sessions")?;
    let start = transcript.len();
    send(b"\x0e")?;
    wait_for(&receiver, &mut transcript, start, "New session")?;
    let start = transcript.len();
    send(b"\x1b[Z\x1b[Z\x1b[Z\x1b[Z\r")?;
    wait_for(&receiver, &mut transcript, start, "Search:")?;
    let start = transcript.len();
    send(format!("\x1b[200~{client_label}\x1b[201~").as_bytes())?;
    wait_for(
        &receiver,
        &mut transcript,
        start,
        &format!("Search: {client_label}"),
    )?;
    let start = transcript.len();
    send(b"\r")?;
    wait_for(
        &receiver,
        &mut transcript,
        start,
        &format!("{client_label} ▾"),
    )?;
    let start = transcript.len();
    send(b"\t\t\t\t\r")?;
    wait_for(&receiver, &mut transcript, start, "CLAUDE_TERMINAL_HANDOFF")?;
    if !child.0.wait()?.success() {
        return Err("lifecycle fixture must exit normally".into());
    }
    drop(writer);
    drop(pair.master);
    let terminal = reading.join().map_err(|_| "terminal reader panicked")??;
    if terminal.flags_at_handoff != Some(vec![0, 5]) {
        return Err(format!(
            "picker must pop its flag layer before the handoff: {:?}",
            terminal.flags_at_handoff
        )
        .into());
    }
    if terminal.stack != vec![0, 5] {
        return Err(format!("keyboard stack changed after handoff: {:?}", terminal.stack).into());
    }
    let text = String::from_utf8_lossy(&terminal.transcript);
    if !text.contains(&format!("CLAUDE_TERMINAL_MODE {expected_mode}")) {
        return Err("terminal handoff lost its typed launch mode".into());
    }
    let calls = std::fs::read_to_string(flow)?;
    let calls = calls.lines().collect::<Vec<_>>();
    match (expected_mode, calls.as_slice()) {
        ("BackgroundThenAttach", [background_start, attach_call])
            if background_start.contains(r#""arguments": ["--bg", "--model", "haiku"]"#)
                && attach_call.contains(r#""arguments": ["attach", "a1b2c3d4"]"#) => {}
        ("Foreground", [foreground_start])
            if foreground_start.contains(r#""arguments": ["--model", "haiku"]"#) => {}
        _ => {
            return Err(
                format!("native arguments or start/attach order changed: {calls:?}").into(),
            );
        }
    }
    let before = Regex::new(r"CLAUDE_TERMIOS_BEFORE ([^\r\n]+)")?
        .captures(&text)
        .ok_or("missing pre-picker termios")?;
    let after = Regex::new(r"CLAUDE_TERMIOS_AFTER ([^\r\n]+)")?
        .captures(&text)
        .ok_or("missing post-picker termios")?;
    if before.get(1).map(|value| value.as_str()) != after.get(1).map(|value| value.as_str()) {
        return Err("termios must match before client dispatch".into());
    }
    let returned = Regex::new(r"CLAUDE_TERMIOS_RETURNED ([^\r\n]+)")?
        .captures(&text)
        .ok_or("missing post-native termios")?;
    if before.get(1).map(|value| value.as_str()) != returned.get(1).map(|value| value.as_str()) {
        return Err("termios must match after native client returns".into());
    }
    Ok(())
}
