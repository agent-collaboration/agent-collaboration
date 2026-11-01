//! First native frame of the declared component fixture, before any resize or key.
use super::capture_support::{digest_file, verified_node_executable};
use super::isolated_fixture_test::{TestResult, ensure};
use super::terminal_interaction_test::TerminalDriver;
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const FIXTURE_ENTRY: &str = "quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture";
const FRAME_BEGIN: &[u8] = b"\x1b[?2026h";
const FRAME_END: &[u8] = b"\x1b[?2026l";
const FRAME_DEADLINE: Duration = Duration::from_secs(8);

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewportSample {
    kernel: Option<[u16; 2]>,
    terminal_hook: [u16; 2],
    observed: [usize; 2],
    drawn: [u16; 2],
    origin: [i16; 2],
}

fn receive_first_frame(terminal: &mut TerminalDriver) -> TestResult<Vec<u8>> {
    let deadline = Instant::now() + FRAME_DEADLINE;
    let mut received = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("first native frame deadline")?;
        let bytes = terminal
            .receive_bytes(remaining)?
            .ok_or("fixture exited before its first completed native frame")?;
        received.extend(bytes);
        if let Some(begin) = received
            .windows(FRAME_BEGIN.len())
            .position(|window| window == FRAME_BEGIN)
        {
            let content_start = begin + FRAME_BEGIN.len();
            if let Some(end) = received
                .get(content_start..)
                .ok_or("native frame begins outside its received bytes")?
                .windows(FRAME_END.len())
                .position(|window| window == FRAME_END)
            {
                received.truncate(content_start + end + FRAME_END.len());
                return Ok(received);
            }
        }
    }
}

fn require_initial_viewport(receipt: &Path) -> TestResult<()> {
    let samples: Vec<ViewportSample> =
        serde_json::from_slice(&fs::read(receipt.with_extension("viewport-trace.json"))?)?;
    let first = samples.first().ok_or("fixture viewport trace is empty")?;
    if first.kernel == Some([120, 36])
        && first.terminal_hook == [120, 36]
        && first.observed == [120, 36]
        && first.drawn == [120, 36]
        && first.origin == [0, 0]
    {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "first fixture viewport must be the initial 120x36 PTY: {first:?}"
        ))
        .into())
    }
}

fn check_first_frame_grid(evidence: &Path, frame: &[u8]) -> TestResult<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let (node, _) = verified_node_executable()?;
    let script = fs::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/verification/tui/journeys/scenes/check-quota-first-frame.ts"),
    )?;
    ensure(
        digest_file(&script)?
            == format!(
                "{:x}",
                Sha256::digest(include_bytes!(
                    "../../../../../../scripts/verification/tui/journeys/scenes/check-quota-first-frame.ts"
                ))
            ),
        "first-frame checker differs from its compiled source",
    )?;
    let mut observer = Command::new(node)
        .arg(script)
        .arg(evidence)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    observer
        .stdin
        .take()
        .ok_or("first-frame observer stdin missing")?
        .write_all(frame)?;
    let result = observer.wait_with_output()?;
    fs::write(evidence.join("observer.stdout"), &result.stdout)?;
    fs::write(evidence.join("observer.stderr"), &result.stderr)?;
    if result.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "first native frame is not the wide quota layout: {}",
            String::from_utf8_lossy(&result.stderr)
        ))
        .into())
    }
}

#[test]
#[ignore = "requires an exact signed component fixture and an exclusive native window"]
fn compiled_component_first_frame_uses_initial_120x36_pty_without_resize() -> TestResult<()> {
    // Arrange: nominate the exact product and a fresh retained evidence directory.
    let binary = fs::canonicalize(
        std::env::var_os("TUI_GRID_COMPONENT_BINARY").ok_or("component binary required")?,
    )?;
    let expected_digest = std::env::var("TUI_GRID_COMPONENT_BINARY_SHA256")?;
    ensure(
        expected_digest.len() == 64 && digest_file(&binary)? == expected_digest,
        "declared component artifact hash changed",
    )?;
    let evidence = PathBuf::from(
        std::env::var_os("TUI_GRID_FIRST_FRAME_EVIDENCE").ok_or("first-frame evidence required")?,
    );
    ensure(
        evidence.is_absolute(),
        "first-frame evidence must be absolute",
    )?;
    fs::create_dir(&evidence)?;
    let receipt = evidence.join("fixture-finished");
    let mut terminal = TerminalDriver::spawn_with_environment(
        &binary,
        [
            FIXTURE_ENTRY,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ],
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &[
            ("QUOTA_COMPONENT_FIXTURE", OsStr::new("normal")),
            ("QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE", receipt.as_os_str()),
        ],
    )?;

    // Act: openpty already set 120x36. Observe its first frame without resize/input.
    let first_frame = receive_first_frame(&mut terminal)?;
    fs::write(evidence.join("first-frame.ansi"), &first_frame)?;
    terminal.send(b"q")?;
    let complete = terminal.finish(FRAME_DEADLINE)?;
    fs::write(evidence.join("complete.ansi"), &complete)?;
    ensure(
        fs::read_to_string(&receipt)?
            == "QUOTA_COMPONENT_FIXTURE_FINISHED scenario=Normal released=false external_requests=0 persisted_writes=0\n",
        "fixture must finish through real Quit with zero external requests and writes",
    )?;
    ensure(
        digest_file(&binary)? == expected_digest,
        "component artifact changed during the first-frame check",
    )?;

    // Assert only after the actual child exited and its retained PTY reached EOF.
    let grid_result = check_first_frame_grid(&evidence, &first_frame);
    let viewport_result = require_initial_viewport(&receipt);
    fs::write(
        evidence.join("first-frame-checks.txt"),
        format!("grid={grid_result:?}\nviewport={viewport_result:?}\nresize_commands=0\n"),
    )?;
    grid_result?;
    viewport_result
}
