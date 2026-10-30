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
    send(b"\x1b[200~Claude Code\x1b[201~")?;
    wait_for(&receiver, &mut transcript, start, "Search: Claude Code")?;
    let start = transcript.len();
    send(b"\r")?;
    wait_for(&receiver, &mut transcript, start, "Claude Code ▾")?;
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
    let before = Regex::new(r"CLAUDE_TERMIOS_BEFORE ([^\r\n]+)")?
        .captures(&text)
        .ok_or("missing pre-picker termios")?;
    let after = Regex::new(r"CLAUDE_TERMIOS_AFTER ([^\r\n]+)")?
        .captures(&text)
        .ok_or("missing post-picker termios")?;
    if before.get(1).map(|value| value.as_str()) != after.get(1).map(|value| value.as_str()) {
        return Err("termios must match before client dispatch".into());
    }
    Ok(())
}
