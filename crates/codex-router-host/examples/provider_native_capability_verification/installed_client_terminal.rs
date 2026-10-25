//! Drive one installed Codex TUI interactively in a private pseudo-terminal.
//!
//! The journey needs real keystrokes: the `$` picker, Tab selection and a typed task,
//! which a prompt argument cannot produce (Codex 0.163 builds a plugin Mention only
//! from a picker binding). Raw bytes are appended to the capture as they arrive. Waits
//! are bounded and look for screen text; a timeout fails instead of passing. Only this
//! driver's own child is ever signalled.
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(crate) const TAB: &[u8] = b"\t";
pub(crate) const ENTER: &[u8] = b"\r";
pub(crate) const ESCAPE: &[u8] = b"\x1b";
pub(crate) const CLEAR_LINE: &[u8] = b"\x15";
const CTRL_C: &[u8] = b"\x03";
/// Quiet time before Enter: more than Codex 0.163's 120 ms paste Enter-suppression window.
const SUBMIT_QUIET: Duration = Duration::from_millis(300);
/// One drive's raw terminal output; past this the capture is incomplete and fails.
pub(crate) const CAPTURE_MAX_BYTES: usize = 64 * 1024 * 1024;
const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";
const SIZE: PtySize = PtySize {
    rows: 50,
    cols: 160,
    pixel_width: 0,
    pixel_height: 0,
};

/// Terminal text with control sequences removed and CRLF normalized.
pub(crate) fn terminal_text(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw).replace("\r\n", "\n");
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        match characters.next() {
            // CSI: parameters and intermediates until a final byte in @..~.
            Some('[') => {
                for next in characters.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: until BEL or ESC \.
            Some(']') => {
                while let Some(next) = characters.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && characters.peek() == Some(&'\\') {
                        characters.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    plain
}

/// Whether any of the words appears case-insensitively as a whole word.
pub(crate) fn contains_word(screen: &str, words: &[&str]) -> bool {
    let lower = screen.to_lowercase();
    words.iter().any(|word| {
        lower.match_indices(word).any(|(start, found)| {
            let before = lower.get(..start).and_then(|text| text.chars().next_back());
            let after = lower
                .get(start + found.len()..)
                .and_then(|text| text.chars().next());
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    })
}

/// Codex 0.163's saved folder-trust dialog, as its TUI draws it with cursor moves in
/// place of spaces ("Trust this folder? … Your trust decision will be saved."). It is
/// drawn after the composer, so a ready composer alone never proves no dialog follows.
const TRUST_DIALOG_MARKERS: [&str; 2] = ["Trustthisfolder?", "Yourtrustdecisionwillbesaved"];

/// Whether the screen shows the saved folder-trust dialog, spacing aside.
pub(crate) fn shows_trust_dialog(screen: &str) -> bool {
    let compact: String = screen
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    TRUST_DIALOG_MARKERS
        .iter()
        .any(|marker| compact.contains(marker))
}

/// Whole-word client error text; the wording itself is recorded, not asserted.
pub(crate) const CLIENT_ERROR_WORDS: [&str; 4] =
    ["error", "unsupported", "rejected", "not supported"];

pub(crate) struct InstalledClientTerminal {
    /// Held so the terminal stays open for the child's lifetime.
    _master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: mpsc::Receiver<Vec<u8>>,
    capture: File,
    raw: Vec<u8>,
    terminal_closed: bool,
    shut_down: bool,
    pub(crate) forced_stop: bool,
    /// Raw evidence is incomplete: an overflow or a failed capture write.
    pub(crate) capture_fault: Option<String>,
}

impl InstalledClientTerminal {
    /// Start the program in a fresh PTY with the inherited user environment plus
    /// `extra_environment`; the capture file must not exist yet.
    pub(crate) fn spawn(
        program: &Path,
        arguments: &[String],
        cwd: &Path,
        extra_environment: &[(&str, String)],
        capture: &Path,
    ) -> Result<Self, String> {
        let capture = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(capture)
            .map_err(|error| format!("capture {} not created: {error}", capture.display()))?;
        let pair = native_pty_system()
            .openpty(SIZE)
            .map_err(|error| format!("pty unavailable: {error}"))?;
        let mut command = CommandBuilder::new(program);
        command.args(arguments);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        for (name, value) in extra_environment {
            command.env(name, value);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("{} did not start: {error}", program.display()))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("pty reader unavailable: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("pty writer unavailable: {error}"))?;
        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            let mut chunk = [0_u8; 8192];
            while let Ok(read) = reader.read(&mut chunk) {
                let Some(bytes) = chunk.get(..read).filter(|bytes| !bytes.is_empty()) else {
                    break;
                };
                if sender.send(bytes.to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            _master: pair.master,
            writer,
            child,
            output,
            capture,
            raw: Vec::new(),
            terminal_closed: false,
            shut_down: false,
            forced_stop: false,
            capture_fault: None,
        })
    }

    pub(crate) fn child_pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub(crate) fn mark(&mut self) -> usize {
        self.pump(Duration::ZERO);
        self.raw.len()
    }

    pub(crate) fn screen_since(&self, mark: usize) -> String {
        terminal_text(self.raw.get(mark..).unwrap_or_default())
    }

    pub(crate) fn trust_dialog_shown(&self) -> bool {
        shows_trust_dialog(&self.screen_since(0))
    }

    /// User input. Refused once the trust dialog appeared, so no keystroke can answer it
    /// and no trust decision is ever saved by this driver.
    pub(crate) fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.pump(Duration::ZERO);
        if self.trust_dialog_shown() {
            return Err(
                "installed Codex showed a saved folder-trust dialog; stopped before any input"
                    .to_owned(),
            );
        }
        self.write_terminal(bytes)
    }

    /// Terminal protocol replies and the quit key, which never answer a dialog choice.
    fn write_terminal(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("pty write failed: {error}"))
    }

    /// Wait for the composer, then keep watching for `settle`: the trust dialog is
    /// drawn after the composer, and any input before it is ruled out would answer it.
    pub(crate) fn wait_for_ready_composer(
        &mut self,
        composer: &str,
        timeout: Duration,
        settle: Duration,
    ) -> Result<(), String> {
        self.wait_for(composer, 0, timeout)?;
        let deadline = Instant::now() + settle;
        while Instant::now() < deadline && !self.terminal_closed {
            self.pump(Duration::from_millis(200));
        }
        if self.trust_dialog_shown() {
            return Err(
                "installed Codex showed a saved folder-trust dialog; stopped before any input"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// Multi-line task text arrives as one bracketed paste, as a user pasting it would.
    pub(crate) fn type_text(&mut self, text: &str) -> Result<(), String> {
        if text.contains('\n') {
            let pasted = format!("\x1b[200~{text}\x1b[201~");
            self.send(pasted.as_bytes())
        } else {
            self.send(text.as_bytes())
        }
    }

    /// Type `text` and submit it with Enter, as a person would: only after the typed text
    /// is drawn and the composer has been quiet past its Enter-suppression window.
    ///
    /// Codex 0.163 treats fast plain typing as a non-bracketed paste burst
    /// (`codex-rs/tui/src/bottom_pane/paste_burst.rs` at 83086188: three characters
    /// within 8 ms start one, and an Enter within `PASTE_ENTER_SUPPRESS_WINDOW`, 120 ms,
    /// after it is inserted as a newline instead of submitting;
    /// `chat_composer.rs:3612-3621`). A PTY writes typed text at once, so an immediate
    /// Enter would never submit. Single-line text must be visible first (the burst has
    /// been flushed); a bracketed paste clears burst state but still waits out the window.
    /// Returns a mark taken just before Enter, so later output is judged after it.
    pub(crate) fn type_and_submit(
        &mut self,
        text: &str,
        timeout: Duration,
    ) -> Result<usize, String> {
        let typed_mark = self.mark();
        self.type_text(text)?;
        if !text.contains('\n') {
            let expected: String = text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            self.wait_until(typed_mark, timeout, |screen| {
                let compact: String = screen
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect();
                compact.contains(&expected).then_some("typed")
            })
            .ok_or_else(|| "typed input never appeared in the composer".to_owned())?;
        }
        let quiet_until = Instant::now() + SUBMIT_QUIET;
        loop {
            let remaining = quiet_until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            self.pump(remaining);
        }
        let mark = self.mark();
        self.send(ENTER)?;
        Ok(mark)
    }

    /// Read what has arrived within `timeout`; answer a cursor-position query as a
    /// top-left cursor would, since no terminal emulator answers here. Once the child
    /// closes the terminal, nothing more can arrive.
    fn pump(&mut self, timeout: Duration) {
        if self.terminal_closed {
            return;
        }
        let mut next = self.output.recv_timeout(timeout);
        loop {
            match next {
                Ok(chunk) => {
                    if self.raw.len() + chunk.len() > CAPTURE_MAX_BYTES {
                        self.capture_fault = Some(format!(
                            "terminal output exceeds its {CAPTURE_MAX_BYTES}-byte bound"
                        ));
                        self.terminal_closed = true;
                        return;
                    }
                    if let Err(error) = self.capture.write_all(&chunk) {
                        self.capture_fault = Some(format!("raw capture write failed: {error}"));
                    }
                    if chunk
                        .windows(CURSOR_POSITION_QUERY.len())
                        .any(|window| window == CURSOR_POSITION_QUERY)
                    {
                        let _ = self.write_terminal(b"\x1b[1;1R");
                    }
                    self.raw.extend_from_slice(&chunk);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.terminal_closed = true;
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => return,
            }
            next = self.output.try_recv().map_err(|error| match error {
                mpsc::TryRecvError::Empty => mpsc::RecvTimeoutError::Timeout,
                mpsc::TryRecvError::Disconnected => mpsc::RecvTimeoutError::Disconnected,
            });
        }
    }

    /// Wait until `classify` labels the normalized screen text since `mark`; None when
    /// the bound expires or the child closed the terminal first.
    fn wait_until(
        &mut self,
        mark: usize,
        timeout: Duration,
        classify: impl Fn(&str) -> Option<&'static str>,
    ) -> Option<&'static str> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(label) = classify(&self.screen_since(mark)) {
                return Some(label);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || self.terminal_closed {
                return None;
            }
            self.pump(remaining.min(Duration::from_millis(200)));
        }
    }

    /// Wait for `needle` in the normalized screen text since `mark`.
    pub(crate) fn wait_for(
        &mut self,
        needle: &str,
        mark: usize,
        timeout: Duration,
    ) -> Result<String, String> {
        self.wait_until(mark, timeout, |screen| {
            screen.contains(needle).then_some("expected")
        })
        .map(|_| self.screen_since(mark))
        .ok_or_else(|| format!("screen never showed {needle:?}"))
    }

    /// `effect` when the screen shows the effect text, `error` when it shows client
    /// error words first, None when the bound expires.
    pub(crate) fn wait_for_effect_or_error(
        &mut self,
        effect: &str,
        mark: usize,
        timeout: Duration,
    ) -> Option<&'static str> {
        self.wait_until(mark, timeout, |screen| {
            if screen.contains(effect) {
                Some("effect")
            } else if contains_word(screen, &CLIENT_ERROR_WORDS) {
                Some("error")
            } else {
                None
            }
        })
    }

    fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Quit through the TUI's own Ctrl-C shutdown; force-stop only this child.
    pub(crate) fn shutdown(&mut self, timeout: Duration) -> Result<(), String> {
        if self.shut_down {
            return Ok(());
        }
        self.shut_down = true;
        for _ in 0..2 {
            if self.exited() {
                break;
            }
            let _ = self.write_terminal(CTRL_C);
            let deadline = Instant::now() + timeout / 2;
            while Instant::now() < deadline && !self.exited() {
                self.pump(Duration::from_millis(200));
            }
        }
        if !self.exited() {
            self.forced_stop = true;
            self.child
                .kill()
                .map_err(|error| format!("own child stop failed: {error}"))?;
            self.child
                .wait()
                .map_err(|error| format!("own child wait failed: {error}"))?;
        }
        self.pump(Duration::ZERO);
        Ok(())
    }
}

impl Drop for InstalledClientTerminal {
    fn drop(&mut self) {
        let _ = self.shutdown(Duration::from_secs(10));
    }
}
