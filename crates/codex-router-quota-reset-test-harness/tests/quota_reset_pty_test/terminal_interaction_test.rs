use std::ffi::OsStr;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use portable_pty::Child;
use portable_pty::CommandBuilder;
use portable_pty::MasterPty;
use portable_pty::PtySize;
use portable_pty::native_pty_system;

use super::isolated_fixture_test::FORBIDDEN_TERMINAL_CANARIES;
use super::isolated_fixture_test::TestResult;

#[path = "terminal_failure_capture.rs"]
mod failure_capture;

const INITIAL_SIZE: PtySize = PtySize {
    rows: 36,
    cols: 120,
    pixel_width: 0,
    pixel_height: 0,
};

pub(super) struct TerminalDriver {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    output_receiver: mpsc::Receiver<ReaderEvent>,
    reader_thread: Option<thread::JoinHandle<()>>,
    transcript: Vec<u8>,
    reached_eof: bool,
    public_wrapper: Option<PathBuf>,
    failure_capture: Option<failure_capture::FailureCapture>,
}

enum ReaderEvent {
    Bytes(Vec<u8>),
    Eof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KeyboardProtocol {
    Legacy,
    Kitty,
}

impl TerminalDriver {
    /// Forward each received chunk immediately while retaining the original
    /// event stream for this owner's transcript, EOF and child cleanup methods.
    pub(super) fn forward_output(
        &mut self,
        mut publish: impl FnMut(Option<&[u8]>) -> std::io::Result<()> + Send + 'static,
    ) -> mpsc::Receiver<std::io::Result<()>> {
        let (retained_sender, retained_receiver) = mpsc::channel();
        let incoming = std::mem::replace(&mut self.output_receiver, retained_receiver);
        let reader = self.reader_thread.take();
        let (completion, completed) = mpsc::channel();
        self.reader_thread = Some(thread::spawn(move || {
            let result = (|| {
                while let Ok(event) = incoming.recv() {
                    let eof = matches!(event, ReaderEvent::Eof);
                    publish(match &event {
                        ReaderEvent::Bytes(bytes) => Some(bytes),
                        ReaderEvent::Eof => None,
                    })?;
                    if retained_sender.send(event).is_err() || eof {
                        return Ok(());
                    }
                }
                Err(std::io::Error::other("PTY output disconnected before EOF"))
            })();
            // Existing cleanup joins this handle before draining its retained
            // transcript. Chaining the original reader prevents an orphan and
            // ensures every final chunk is forwarded before that drain.
            let reader_result = reader.map_or(Ok(()), |reader| {
                reader
                    .join()
                    .map_err(|_| std::io::Error::other("PTY reader thread panicked"))
            });
            let _ = completion.send(result.and(reader_result));
        }));
        completed
    }

    /// Receive actual PTY bytes before the observer chooses its next input.
    /// None is EOF; a deadline remains an error, never a synthetic empty frame.
    pub(super) fn receive_bytes(&mut self, timeout: Duration) -> TestResult<Option<Vec<u8>>> {
        if self.reached_eof {
            return Ok(None);
        }
        match self.receive_event(timeout)? {
            ReaderEvent::Bytes(bytes) => {
                self.transcript.extend_from_slice(&bytes);
                Ok(Some(bytes))
            }
            ReaderEvent::Eof => {
                self.reached_eof = true;
                Ok(None)
            }
        }
    }

    /// Replies originate in the emulator, rather than a fabricated query answer.
    /// Legacy observations deliberately leave the kitty query unanswered.
    pub(super) fn send_terminal_reply(
        &mut self,
        reply: &[u8],
        protocol: KeyboardProtocol,
    ) -> TestResult<()> {
        if protocol == KeyboardProtocol::Legacy
            && reply.starts_with(b"\x1b[?")
            && reply.ends_with(b"u")
        {
            return Ok(());
        }
        self.send(reply)
    }
    pub(super) fn spawn(
        program: &Path,
        arguments: impl IntoIterator<Item = impl AsRef<OsStr>>,
        current_directory: &Path,
    ) -> TestResult<Self> {
        Self::spawn_with_no_color(program, arguments, current_directory, false)
    }

    pub(super) fn spawn_with_no_color(
        program: &Path,
        arguments: impl IntoIterator<Item = impl AsRef<OsStr>>,
        current_directory: &Path,
        no_color: bool,
    ) -> TestResult<Self> {
        let environment = if no_color {
            vec![("NO_COLOR", OsStr::new("1"))]
        } else {
            Vec::new()
        };
        Self::spawn_with_environment(program, arguments, current_directory, &environment)
    }

    pub(super) fn spawn_with_environment(
        program: &Path,
        arguments: impl IntoIterator<Item = impl AsRef<OsStr>>,
        current_directory: &Path,
        environment: &[(&str, &OsStr)],
    ) -> TestResult<Self> {
        let mut command = CommandBuilder::new(program);
        command.args(arguments);
        command.env_clear();
        command.env("HOME", std::env::temp_dir());
        command.env("TERM", "xterm-256color");
        command.env("LANG", "C.UTF-8");
        command.env("TMPDIR", std::env::temp_dir());
        for (key, value) in environment {
            if !matches!(
                *key,
                "NO_COLOR"
                    | "TUI_SESSIONS_FIXTURE_RECEIPT_DIR"
                    | "CODEX_ROUTER_TUI_STORYBOARD_VARIANT"
                    | "QUOTA_COMPONENT_FIXTURE"
                    | "QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE"
                    | "QUOTA_COMPONENT_FIXTURE_RELEASE_FILE"
            ) {
                return Err(std::io::Error::other("unapproved PTY fixture environment key").into());
            }
            command.env(key, value);
        }
        command.cwd(current_directory);
        Self::spawn_prepared_command(command, None)
    }

    pub(super) fn spawn_prepared_command(
        command: CommandBuilder,
        public_wrapper: Option<PathBuf>,
    ) -> TestResult<Self> {
        let pair = native_pty_system().openpty(INITIAL_SIZE)?;
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (output_sender, output_receiver) = mpsc::channel();
        let reader_thread = thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(bytes) => {
                        let Some(bytes) = chunk.get(..bytes) else {
                            break;
                        };
                        if output_sender
                            .send(ReaderEvent::Bytes(bytes.to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) if error.raw_os_error() == Some(5) => break,
                    Err(_error) => break,
                }
            }
            let _ = output_sender.send(ReaderEvent::Eof);
        });
        Ok(Self {
            master: Some(pair.master),
            writer: Some(writer),
            child: Some(child),
            output_receiver,
            reader_thread: Some(reader_thread),
            transcript: Vec::new(),
            reached_eof: false,
            public_wrapper,
            failure_capture: failure_capture::FailureCapture::from_environment(),
        })
    }

    pub(super) fn wait_for_text(&mut self, expected: &str, timeout: Duration) -> TestResult<()> {
        self.wait_until(timeout, |transcript| {
            String::from_utf8_lossy(transcript).contains(expected)
        })
    }

    pub(super) fn transcript_len(&mut self) -> usize {
        self.drain_pending_output();
        self.transcript.len()
    }

    pub(super) fn safe_semantic_diagnostics(&mut self, start: usize) -> String {
        self.drain_pending_output();
        let tail = self.transcript.get(start..).unwrap_or_default();
        let terminal_bytes = String::from_utf8_lossy(tail);
        let child_state = match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => format!("exited({})", status.exit_code()),
                Ok(None) => "running".to_owned(),
                Err(error) => format!("status-unavailable({error})"),
            },
            None => "released".to_owned(),
        };
        let reader_finished = self
            .reader_thread
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished);
        let tail_preview = safe_terminal_tail(tail);
        format!(
            "tail_bytes={} fullscreen_entered={} keyboard_probe={} device_attributes_probe={} mouse_capture_enabled={} synchronized_update_started={} inspecting_footer={} browse_footer={} reset_title={} eof={} child_state={} reader_finished={} tail_preview={tail_preview:?}",
            tail.len(),
            terminal_bytes.contains("\u{1b}[?1049h"),
            terminal_bytes.contains("\u{1b}[?u"),
            terminal_bytes.contains("\u{1b}[c"),
            terminal_bytes.contains("\u{1b}[?1003h") && terminal_bytes.contains("\u{1b}[?1006h"),
            terminal_bytes.contains("\u{1b}[?2026h"),
            terminal_bytes.contains("esc/ctrl-r back"),
            terminal_bytes.contains("ctrl-r account options"),
            terminal_bytes.contains("Reset credit"),
            self.reached_eof,
            child_state,
            reader_finished,
        )
    }

    pub(super) fn wait_for_text_after(
        &mut self,
        expected: &str,
        start: usize,
        timeout: Duration,
    ) -> TestResult<()> {
        self.wait_until(timeout, |transcript| {
            transcript.get(start..).is_some_and(|tail| {
                contains_completed_synchronized_frame_after_text(tail, expected.as_bytes())
            })
        })
        .map_err(|error| {
            let diagnostics = self.safe_semantic_diagnostics(start);
            std::io::Error::other(format!(
                "PTY action acknowledgement expected={expected:?} action_start={start}: {error}; {diagnostics}"
            ))
            .into()
        })
    }

    /// Match a styled label/value on one rendered row, after this action only.
    pub(super) fn wait_for_styled_text_after(
        &mut self,
        expected: &str,
        start: usize,
        timeout: Duration,
    ) -> TestResult<()> {
        self.wait_until(timeout, |transcript| {
            transcript
                .get(start..)
                .is_some_and(|tail| contains_styled_completed_frame(tail, expected))
        })
    }

    pub(super) fn resize(&mut self, rows: u16, cols: u16) -> TestResult<()> {
        if self.failure_capture.is_some() {
            self.drain_pending_output();
        }
        self.master_ref()?.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        if let Some(capture) = self.failure_capture.as_mut() {
            capture.record_resize(self.transcript.len(), cols, rows);
        }
        Ok(())
    }

    pub(super) fn send(&mut self, bytes: &[u8]) -> TestResult<()> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| std::io::Error::other("PTY writer is unavailable"))?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub(super) fn send_sgr_mouse_left_down(&mut self, column: u16, row: u16) -> TestResult<()> {
        let sequence = format!("\u{1b}[<0;{};{}M", column + 1, row + 1);
        self.send(sequence.as_bytes())
    }

    pub(super) fn child_is_running(&mut self) -> TestResult<bool> {
        Ok(self
            .child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("PTY child is unavailable"))?
            .try_wait()?
            .is_none())
    }

    pub(super) fn finish(mut self, timeout: Duration) -> TestResult<Vec<u8>> {
        if let Err(error) = self.wait_for_eof(timeout) {
            let diagnostics = self.safe_semantic_diagnostics(0);
            return Err(std::io::Error::other(format!(
                "PTY EOF wait failed: {error}; {diagnostics}"
            ))
            .into());
        }
        let status = self
            .child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("PTY child is unavailable"))?
            .wait()?;
        if !status.success() {
            return Err(std::io::Error::other(format!(
                "PTY child failed with exit code {}",
                status.exit_code()
            ))
            .into());
        }
        self.child.take();
        self.writer.take();
        self.master.take();
        if let Some(reader_thread) = self.reader_thread.take() {
            reader_thread
                .join()
                .map_err(|_panic| std::io::Error::other("PTY reader thread panicked"))?;
        }
        Ok(std::mem::take(&mut self.transcript))
    }

    pub(super) fn terminate_and_reap(mut self, timeout: Duration) -> TestResult<Vec<u8>> {
        if self.public_wrapper.is_some() {
            return self.terminate_public_wrapper(timeout);
        }
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("PTY child is unavailable"))?;
        if child.try_wait()?.is_none() {
            child.kill()?;
        }
        let deadline = Instant::now() + timeout;
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                self.capture_wait_failure();
                return Err(std::io::Error::other("PTY child termination exceeded timeout").into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.child.take();
        self.writer.take();
        self.master.take();
        if let Some(reader_thread) = self.reader_thread.take() {
            reader_thread
                .join()
                .map_err(|_panic| std::io::Error::other("PTY reader thread panicked"))?;
        }
        while let Ok(event) = self.output_receiver.try_recv() {
            if let ReaderEvent::Bytes(bytes) = event {
                self.transcript.extend(bytes);
            }
        }
        Ok(std::mem::take(&mut self.transcript))
    }

    pub(super) fn signal_public_wrapper(&mut self) -> TestResult<()> {
        let output = self
            .public_wrapper
            .as_ref()
            .ok_or("public wrapper binding absent")?;
        let child = self.child.as_mut().ok_or("public wrapper child absent")?;
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        let pid = child.process_id().ok_or("public wrapper PID absent")?;
        let recorded: u32 = std::fs::read_to_string(output.join("quota-wrapper.pid"))?
            .trim()
            .parse()?;
        if recorded != pid {
            return Err("public wrapper PID differs from retained child".into());
        }
        // The receipt writer pins LC_ALL=C too, so `lstart` bytes cannot differ by parent locale.
        let snapshot = std::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "uid=,lstart=,command="])
            .env("LC_ALL", "C")
            .output()?;
        let identity = std::fs::read(output.join("quota-wrapper.identity"))?;
        let command = String::from_utf8_lossy(&snapshot.stdout);
        if !snapshot.status.success()
            || snapshot.stdout != identity
            || !command.contains(
                output
                    .join("fixture-command.json")
                    .to_string_lossy()
                    .as_ref(),
            )
        {
            return Err("public wrapper UID/birth/command changed; refusing signal".into());
        }
        let signal = std::process::Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .output()?;
        if !signal.status.success() {
            return Err("public wrapper SIGTERM failed".into());
        }
        Ok(())
    }

    fn terminate_public_wrapper(&mut self, timeout: Duration) -> TestResult<Vec<u8>> {
        self.signal_public_wrapper()?;
        self.wait_for_eof(timeout)?;
        let child = self.child.as_mut().ok_or("public wrapper unavailable")?;
        let deadline = Instant::now() + timeout;
        while child.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                self.capture_wait_failure();
                return Err("public wrapper cleanup unsettled".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.child.take();
        self.writer.take();
        self.master.take();
        if let Some(reader) = self.reader_thread.take() {
            reader.join().map_err(|_| "public PTY reader panic")?;
        }
        Ok(std::mem::take(&mut self.transcript))
    }

    fn master_ref(&self) -> TestResult<&(dyn MasterPty + Send)> {
        self.master
            .as_deref()
            .ok_or_else(|| std::io::Error::other("PTY master is unavailable").into())
    }

    fn wait_for_eof(&mut self, timeout: Duration) -> TestResult<()> {
        let deadline = Instant::now() + timeout;
        while !self.reached_eof {
            let remaining = deadline.saturating_duration_since(Instant::now());
            self.receive_bytes(remaining)?;
        }
        Ok(())
    }

    fn wait_until(
        &mut self,
        timeout: Duration,
        condition: impl Fn(&[u8]) -> bool,
    ) -> TestResult<()> {
        let deadline = Instant::now() + timeout;
        while !condition(&self.transcript) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.receive_event(remaining)? {
                ReaderEvent::Bytes(bytes) => self.transcript.extend(bytes),
                ReaderEvent::Eof => self.reached_eof = true,
            }
            if self.reached_eof && !condition(&self.transcript) {
                self.capture_wait_failure();
                return Err(std::io::Error::other(
                    "PTY child exited before expected semantic output",
                )
                .into());
            }
        }
        Ok(())
    }

    fn drain_pending_output(&mut self) {
        while let Ok(event) = self.output_receiver.try_recv() {
            match event {
                ReaderEvent::Bytes(bytes) => self.transcript.extend(bytes),
                ReaderEvent::Eof => self.reached_eof = true,
            }
        }
    }
}

fn safe_terminal_tail(transcript: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(transcript).into_owned();
    for canary in FORBIDDEN_TERMINAL_CANARIES {
        text = text.replace(canary, "[redacted]");
    }
    let visible = text
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    visible
        .chars()
        .rev()
        .take(240)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn contains_completed_synchronized_frame_after_text(transcript: &[u8], expected: &[u8]) -> bool {
    if expected.is_empty() {
        return false;
    }

    const BEGIN: &[u8] = b"\x1b[?2026h";
    const END: &[u8] = b"\x1b[?2026l";

    let mut offset = 0;
    let mut synchronized_update_depth = 0;
    let mut expected_text_seen_in_update = false;
    while offset < transcript.len() {
        let remaining = transcript.get(offset..).unwrap_or_default();
        let next_text = find_sequence(remaining, expected).map(|position| offset + position);
        let next_begin = find_sequence(remaining, BEGIN).map(|position| offset + position);
        let next_end = find_sequence(remaining, END).map(|position| offset + position);
        let next_event = [next_text, next_begin, next_end]
            .into_iter()
            .flatten()
            .min();

        let Some(event_offset) = next_event else {
            break;
        };
        if next_text == Some(event_offset) {
            expected_text_seen_in_update |= synchronized_update_depth > 0;
            offset = event_offset + expected.len();
        } else if next_begin == Some(event_offset) {
            if synchronized_update_depth == 0 {
                expected_text_seen_in_update = false;
            }
            synchronized_update_depth += 1;
            offset = event_offset + BEGIN.len();
        } else {
            if synchronized_update_depth > 0 {
                synchronized_update_depth -= 1;
                if synchronized_update_depth == 0 && expected_text_seen_in_update {
                    return true;
                }
                if synchronized_update_depth == 0 {
                    expected_text_seen_in_update = false;
                }
            }
            offset = event_offset + END.len();
        }
    }

    false
}

fn contains_styled_completed_frame(transcript: &[u8], expected: &str) -> bool {
    // Strip only SGR. Cursor movement, row boundaries and synchronized-frame
    // delimiters remain, so a neighbor/background value cannot complete a label.
    let mut output = Vec::new();
    let mut offset = 0;
    let mut last_space = false;
    while let Some(byte) = transcript.get(offset).copied() {
        if byte == 0x1b && transcript.get(offset + 1) == Some(&b'[') {
            let mut end = offset + 2;
            while transcript
                .get(end)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b';')
            {
                end += 1;
            }
            if transcript.get(end) == Some(&b'm') {
                offset = end + 1;
                continue;
            }
        }
        if byte != b' ' || !last_space {
            output.push(byte);
        }
        last_space = byte == b' ';
        offset += 1;
    }
    contains_completed_synchronized_frame_after_text(&output, expected.as_bytes())
}

#[test]
fn styled_pty_acknowledgement_requires_labeled_value_in_same_completed_row() {
    assert!(contains_styled_completed_frame(
        b"\x1b[?2026hSession ID:\x1b[0m        \x1b[37mthread-b\x1b[?2026l",
        "Session ID: thread-b"
    ));
    for wrong in [
        b"\x1b[?2026hSession ID: thread-a\x1b[12;1Hthread-b\x1b[?2026l".as_slice(),
        b"\x1b[?2026hSession ID:\x1b[12;1H thread-b\x1b[?2026l",
        b"\x1b[?2026hSession ID: thread-b",
    ] {
        assert!(!contains_styled_completed_frame(
            wrong,
            "Session ID: thread-b"
        ));
    }
}

fn find_sequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

impl Drop for TerminalDriver {
    fn drop(&mut self) {
        if self.public_wrapper.is_some() && self.child.is_some() {
            if let Err(error) = self.terminate_public_wrapper(Duration::from_secs(8)) {
                if let Some(output) = self.public_wrapper.as_ref() {
                    let _ = std::fs::write(output.join("cleanup-unsettled"), error.to_string());
                }
                self.child.take();
                self.reader_thread.take();
            }
            return;
        }
        if let Some(child) = self.child.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        self.child.take();
        self.writer.take();
        self.master.take();
        if let Some(reader_thread) = self.reader_thread.take() {
            let _ = reader_thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::isolated_fixture_test::ensure;
    use super::*;

    #[test]
    fn output_forwarding_drains_without_a_read_command_and_preserves_transcript() -> TestResult<()>
    {
        let (incoming, output_receiver) = mpsc::channel();
        let (published, publication) = mpsc::channel();
        let mut terminal = TerminalDriver {
            master: None,
            writer: Some(Box::new(Vec::<u8>::new())),
            child: None,
            output_receiver,
            reader_thread: None,
            transcript: Vec::new(),
            reached_eof: false,
            public_wrapper: None,
            failure_capture: None,
        };
        let completed = terminal.forward_output(move |bytes| {
            published
                .send(bytes.map(<[u8]>::to_vec))
                .map_err(|error| std::io::Error::other(error.to_string()))
        });
        let first = b"stale frame".to_vec();
        incoming.send(ReaderEvent::Bytes(first.clone()))?;
        ensure(
            publication.recv_timeout(Duration::from_secs(8))? == Some(first.clone()),
            "stream did not publish before any receive_bytes call",
        )?;
        // The command-side writer remains available while the output worker
        // is blocked causally on the next actual event, without timing polls.
        terminal.send(b"command during burst")?;
        let second = b"post-input frame and marker".to_vec();
        incoming.send(ReaderEvent::Bytes(second.clone()))?;
        incoming.send(ReaderEvent::Eof)?;
        ensure(
            publication.recv_timeout(Duration::from_secs(8))? == Some(second.clone()),
            "stream changed burst order",
        )?;
        ensure(
            publication.recv_timeout(Duration::from_secs(8))?.is_none(),
            "stream did not publish EOF",
        )?;
        completed.recv_timeout(Duration::from_secs(8))??;
        ensure(
            terminal.receive_bytes(Duration::ZERO)? == Some(first.clone()),
            "first transcript chunk lost",
        )?;
        ensure(
            terminal.receive_bytes(Duration::ZERO)? == Some(second.clone()),
            "second transcript chunk lost",
        )?;
        ensure(
            terminal.receive_bytes(Duration::ZERO)?.is_none(),
            "retained EOF lost",
        )?;
        ensure(
            terminal.transcript == [first, second].concat(),
            "forwarding changed retained transcript",
        )
    }

    #[test]
    fn incremental_reads_preserve_actual_bytes_and_only_legacy_skips_kitty_reply() -> TestResult<()>
    {
        let (sender, receiver) = mpsc::channel();
        let bytes = b"\x1b[?u\x1b[c".to_vec();
        sender.send(ReaderEvent::Bytes(bytes.clone()))?;
        sender.send(ReaderEvent::Eof)?;
        let mut terminal = TerminalDriver {
            master: None,
            writer: None,
            child: None,
            output_receiver: receiver,
            reader_thread: None,
            transcript: Vec::new(),
            reached_eof: false,
            public_wrapper: None,
            failure_capture: None,
        };
        ensure(
            terminal.receive_bytes(Duration::ZERO)? == Some(bytes.clone()),
            "incremental read changed actual bytes",
        )?;
        ensure(
            terminal.transcript == bytes,
            "incremental read did not retain transcript",
        )?;
        ensure(
            terminal.receive_bytes(Duration::ZERO)?.is_none(),
            "reader EOF was not preserved",
        )?;
        terminal.send_terminal_reply(b"\x1b[?0u", KeyboardProtocol::Legacy)?;
        ensure(
            terminal
                .send_terminal_reply(b"\x1b[?0u", KeyboardProtocol::Kitty)
                .is_err(),
            "kitty reply was wrongly skipped",
        )?;
        ensure(
            terminal
                .send_terminal_reply(b"\x1b[?62;c", KeyboardProtocol::Legacy)
                .is_err(),
            "legacy device reply was wrongly skipped",
        )?;
        Ok(())
    }

    #[test]
    fn transcript_boundary_includes_bytes_already_queued_by_pty_reader() -> TestResult<()> {
        let (output_sender, output_receiver) = mpsc::channel();
        let queued_frame = b"Reset credit".to_vec();
        output_sender.send(ReaderEvent::Bytes(queued_frame.clone()))?;
        let mut terminal = TerminalDriver {
            master: None,
            writer: None,
            child: None,
            output_receiver,
            reader_thread: None,
            transcript: Vec::new(),
            reached_eof: false,
            public_wrapper: None,
            failure_capture: None,
        };

        if terminal.transcript_len() != queued_frame.len() {
            return Err(std::io::Error::other(
                "transcript boundary excluded bytes already queued by PTY reader",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn action_boundary_wait_rejects_stale_save_and_incomplete_current_frames() -> TestResult<()> {
        for (current_output, should_complete) in [
            (b"".as_slice(), false),
            (b"\x1b[?2026hSave".as_slice(), false),
            (b"\x1b[?2026hSave\x1b[?2026l".as_slice(), true),
        ] {
            let (output_sender, output_receiver) = mpsc::channel();
            let mut terminal = TerminalDriver {
                master: None,
                writer: None,
                child: None,
                output_receiver,
                reader_thread: None,
                transcript: b"\x1b[?2026hSave\x1b[?2026l".to_vec(),
                reached_eof: false,
                public_wrapper: None,
                failure_capture: None,
            };
            let action_start = terminal.transcript_len();
            output_sender.send(ReaderEvent::Bytes(current_output.to_vec()))?;
            output_sender.send(ReaderEvent::Eof)?;

            let completed = terminal
                .wait_for_text_after("Save", action_start, Duration::ZERO)
                .is_ok();

            if completed != should_complete {
                return Err(std::io::Error::other(
                    "action acknowledgement reused old Save or accepted an incomplete repaint",
                )
                .into());
            }
        }
        Ok(())
    }
}
