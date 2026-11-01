//! Opt-in evidence from failed causal waits; normal fixture behaviour stays unchanged.

use super::{INITIAL_SIZE, ReaderEvent, TerminalDriver, TestResult};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KernelResize {
    byte_offset: usize,
    columns: u16,
    rows: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureGeometry<'capture> {
    initial_columns: u16,
    initial_rows: u16,
    resizes: &'capture [KernelResize],
}

pub(super) struct FailureCapture {
    directory: PathBuf,
    resizes: Vec<KernelResize>,
}

impl FailureCapture {
    pub(super) fn from_environment() -> Option<Self> {
        std::env::var_os("CODEX_ROUTER_PTY_CAPTURE_DIR").map(|directory| Self {
            directory: directory.into(),
            resizes: Vec::new(),
        })
    }

    pub(super) fn record_resize(&mut self, byte_offset: usize, columns: u16, rows: u16) {
        self.resizes.push(KernelResize {
            byte_offset,
            columns,
            rows,
        });
    }

    fn retain(&self, transcript: &[u8]) -> TestResult<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)?;
        let thread = std::thread::current();
        let test_name = thread
            .name()
            .unwrap_or("unnamed-test")
            .rsplit("::")
            .next()
            .unwrap_or("unnamed-test");
        let test_name = test_name
            .chars()
            .take(120)
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let prefix = self
            .directory
            .join(format!("{test_name}-pid-{}-{sequence}", std::process::id()));
        let ansi = prefix.with_extension("ansi");
        write_new(&ansi, transcript)?;
        let geometry = CaptureGeometry {
            initial_columns: INITIAL_SIZE.cols,
            initial_rows: INITIAL_SIZE.rows,
            resizes: &self.resizes,
        };
        write_new(
            &prefix.with_extension("json"),
            &serde_json::to_vec_pretty(&geometry)?,
        )?;
        let node = PathBuf::from(
            std::env::var_os("QUOTA_STORYBOARD_NODE")
                .ok_or("failure capture requires the qualified Node executable")?,
        );
        let expected_node_hash = std::env::var("QUOTA_STORYBOARD_NODE_SHA256")?;
        if !node.is_absolute()
            || format!("{:x}", Sha256::digest(fs::read(&node)?)) != expected_node_hash
        {
            return Err("failure capture Node binding differs from its qualification".into());
        }
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/verification/tui/render-pty-failure-grid.ts");
        if fs::read(&script)?
            != include_bytes!(
                "../../../../../../scripts/verification/tui/render-pty-failure-grid.ts"
            )
        {
            return Err("failure capture renderer differs from its compiled source binding".into());
        }
        let output = Command::new(node)
            .env_clear()
            .arg(script)
            .arg(&prefix)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "failure grid renderer failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        eprintln!("PTY failure evidence: {}", ansi.display());
        Ok(())
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)
}

impl TerminalDriver {
    pub(super) fn capture_wait_failure(&mut self) {
        if self.failure_capture.is_none() {
            return;
        }
        self.drain_pending_output();
        if let Some(capture) = &self.failure_capture
            && let Err(error) = capture.retain(&self.transcript)
        {
            eprintln!("PTY failure capture unavailable: {error}");
        }
    }

    pub(super) fn receive_event(&mut self, timeout: Duration) -> TestResult<ReaderEvent> {
        match self.output_receiver.recv_timeout(timeout) {
            Ok(event) => Ok(event),
            Err(error) => {
                self.capture_wait_failure();
                Err(error.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ReaderEvent, TerminalDriver, TestResult};
    use std::{
        fs,
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };

    const CAPTURE_DIRECTORY_ENV: &str = "CODEX_ROUTER_PTY_CAPTURE_DIR";
    const CHILD_ENV: &str = "CODEX_ROUTER_PTY_CAPTURE_WAIT_CHILD";
    const FRAME: &[u8] = b"\x1b[2J\x1b[H\x1b[?2026hCAPTURE_WAIT_SENTINEL\x1b[?2026l";

    struct CaptureTestDirectory(std::path::PathBuf);
    impl Drop for CaptureTestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions fail the test while fixture setup errors propagate."
    )]
    fn forced_wait_failure_retains_evidence_only_when_capture_is_enabled() -> TestResult<()> {
        let directory = CaptureTestDirectory(
            std::env::temp_dir().join(format!("codex-router-capture-check-{}", std::process::id())),
        );
        fs::create_dir(&directory.0)?;
        let module = module_path!()
            .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
            .ok_or("test module must be inside its libtest crate")?;
        let child_test = format!("{module}::forced_wait_failure_capture_child");
        for enabled in [false, true] {
            let output_directory = directory.0.join(if enabled { "enabled" } else { "unset" });
            fs::create_dir(&output_directory)?;
            let mut command = Command::new(std::env::current_exe()?);
            command.args(["--exact", &child_test, "--ignored", "--nocapture"]);
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            command
                .env(CHILD_ENV, "1")
                .env_remove(CAPTURE_DIRECTORY_ENV);
            if enabled {
                command.env(CAPTURE_DIRECTORY_ENV, &output_directory);
            }
            let child = command.spawn()?;
            let child_pid = child.id();
            let output = child.wait_with_output()?;
            assert!(
                output.status.success(),
                "forced wait child failed: {output:?}"
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "the owned child must execute its exact probe: {output:?}"
            );
            let files = fs::read_dir(&output_directory)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<Result<Vec<_>, _>>()?;
            if enabled {
                let transcripts = files
                    .iter()
                    .filter(|file| {
                        file.extension()
                            .is_some_and(|extension| extension == "ansi")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    transcripts.len(),
                    1,
                    "failed wait must retain one original transcript: {files:?}"
                );
                assert_eq!(fs::read(transcripts[0])?, FRAME);
                let filename = transcripts[0]
                    .file_name()
                    .ok_or("transcript filename")?
                    .to_string_lossy();
                assert!(
                    filename.contains("forced_wait_failure_capture_child")
                        && filename.contains(&child_pid.to_string()),
                    "capture identity: {filename}"
                );
                let grid = files
                    .iter()
                    .find(|file| file.extension().is_some_and(|extension| extension == "txt"))
                    .expect("failed wait must render its grid");
                assert!(
                    fs::read_to_string(grid)?.contains("CAPTURE_WAIT_SENTINEL"),
                    "the actual grid retains the failed wait's received frame"
                );
            } else {
                assert!(
                    files.is_empty(),
                    "unset capture must write nothing: {files:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "owned subprocess verifies the capture environment without changing parent process state"]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions fail the child probe while fixture setup errors propagate."
    )]
    fn forced_wait_failure_capture_child() -> TestResult<()> {
        assert_eq!(std::env::var(CHILD_ENV).as_deref(), Ok("1"));
        let (sender, output_receiver) = mpsc::channel();
        sender.send(ReaderEvent::Bytes(FRAME.to_vec()))?;
        drop(sender);
        let mut terminal = TerminalDriver {
            master: None,
            writer: None,
            child: None,
            output_receiver,
            reader_thread: None,
            transcript: Vec::new(),
            reached_eof: false,
            public_wrapper: None,
            failure_capture: super::FailureCapture::from_environment(),
        };
        assert!(
            terminal
                .wait_for_text("ABSENT_CAUSAL_MARKER", Duration::ZERO)
                .is_err()
        );
        Ok(())
    }
}
