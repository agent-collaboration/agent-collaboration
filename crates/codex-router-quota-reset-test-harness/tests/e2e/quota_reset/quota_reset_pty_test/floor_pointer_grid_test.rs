// Real mouse input targets a cell observed from the actual PTY bytes.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FloorIncrementCell {
    column: u16,
    row: u16,
}

fn observe_floor_increment_cell(
    terminal: &mut TerminalDriver,
    evidence: &Path,
) -> TestResult<FloorIncrementCell> {
    use std::io::Write;
    use std::process::Stdio;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = repository.join("scripts/verification/tui/journeys/scenes/locate-floor-increment.ts");
    let mut observer = Command::new("node")
        .arg(script)
        .args(["120", "36"])
        .arg(evidence)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    observer
        .stdin
        .take()
        .ok_or("grid observer stdin unavailable")?
        .write_all(&terminal.transcript_snapshot())?;
    let output = observer.wait_with_output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "floor grid observer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let increment: FloorIncrementCell = serde_json::from_slice(&output.stdout)?;
    ensure(
        increment.column < 120 && increment.row < 36,
        "observed floor control is outside the real viewport",
    )?;
    fs::write(evidence.join("click-cell.json"), &output.stdout)?;
    Ok(increment)
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_quota_tui_floor_arrow_handles_real_mouse_click() -> TestResult<()> {
    let fixture = QuotaResetFixture::create().await?;
    let provider = HeldLoopbackProvider::bind()?;
    let arguments = [
        OsString::from("--router-root"),
        fixture.root().as_os_str().to_owned(),
        OsString::from("--fixture-capability"),
        OsString::from(fixture.capability()),
        OsString::from("--provider-listener"),
        OsString::from(provider.address().to_string()),
    ];
    let mut terminal = TerminalDriver::spawn(
        Path::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness")),
        arguments,
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )?;
    terminal.resize(36, 120)?;

    stage(
        terminal.wait_for_text("Filters", SEMANTIC_WAIT),
        "initial floor browse",
    )?;
    let open_start = terminal.transcript_len();
    terminal.send(&[0x05])?;
    stage(
        terminal.wait_for_text_after("Weekly floor", open_start, SEMANTIC_WAIT),
        "weekly-floor editor",
    )?;
    let evidence = std::env::var_os("QUOTA_FLOOR_PTY_EVIDENCE_DIR")
        .map_or_else(|| fixture.root().join("floor-control-grid"), PathBuf::from);
    fs::create_dir(&evidence)?;
    let increment = observe_floor_increment_cell(&mut terminal, &evidence)?;
    let click_start = terminal.transcript_len();
    terminal.send_sgr_mouse_left_down(increment.column, increment.row)?;
    stage(
        terminal.wait_for_text_after("1%", click_start, SEMANTIC_WAIT),
        "pointer-incremented floor",
    )?;
    let cancel_start = terminal.transcript_len();
    terminal.send(b"\x1b")?;
    stage(
        terminal.wait_for_text_after("Filters", cancel_start, SEMANTIC_WAIT),
        "cancelled weekly-floor editor",
    )?;
    terminal.send(b"q")?;

    let transcript = terminal.finish(SEMANTIC_WAIT)?;
    fs::write(evidence.join("complete.ansi"), &transcript)?;
    let request_records = provider.finish()?;
    ensure(
        request_records.is_empty(),
        "weekly-floor pointer editing unexpectedly contacted the provider",
    )?;
    fixture.assert_read_only()?;
    let transcript = String::from_utf8_lossy(&transcript);
    ensure(
        transcript.contains("\u{1b}[?1003h") && transcript.contains("\u{1b}[?1006h"),
        "floor-button pointer test did not run with mouse capture enabled",
    )?;
    assert_no_printable_output_after_terminal_restoration(transcript.as_bytes())?;
    Ok(())
}
