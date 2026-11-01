//! Fresh-process color policy proof for both compiled interactive surfaces.
use super::*;

#[test]
fn fresh_compiled_no_color_child_keeps_details_and_input_without_color_sequences() -> TestResult<()>
{
    for no_color in [false, true] {
        let mut terminal = TerminalDriver::spawn_with_no_color(
            Path::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness")),
            ["--sessions-picker"],
            Path::new(env!("CARGO_MANIFEST_DIR")),
            no_color,
        )?;
        terminal.wait_for_text("3/3 sessions", SEMANTIC_WAIT)?;
        let start = terminal.transcript_len();
        terminal.send(b"\r")?;
        terminal.wait_for_text_after("Session details", start, SEMANTIC_WAIT)?;
        terminal.wait_for_styled_text_after("Session ID: thread-a", start, SEMANTIC_WAIT)?;
        terminal.send(b"\x1b[13;5u")?;
        let transcript = terminal.finish(SEMANTIC_WAIT)?;
        let text = String::from_utf8_lossy(&transcript);
        ensure(
            text.contains("SESSION_PICKER_OUTCOME resume:thread-a"),
            "color policy changed captured Details activation",
        )?;
        let has_color = text.contains("\x1b[38;") || text.contains("\x1b[48;");
        ensure(
            has_color != no_color,
            "fresh child color policy did not match NO_COLOR (including positive-color control)",
        )?;
        ensure(
            text.contains("\x1b[?1006l") && text.contains("\x1b[?1049l"),
            "color policy changed terminal teardown",
        )?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn fresh_compiled_quota_no_color_child_keeps_windows_modals_and_read_only_exit()
-> TestResult<()> {
    for no_color in [false, true] {
        // Arrange: independent process/fixture, with canonical known window facts.
        let fixture = QuotaResetFixture::create_visual_hard_cases().await?;
        let provider = HeldLoopbackProvider::bind()?;
        let arguments = [
            OsString::from("--router-root"),
            fixture.root().as_os_str().to_owned(),
            OsString::from("--fixture-capability"),
            OsString::from(fixture.capability()),
            OsString::from("--provider-listener"),
            OsString::from(provider.address().to_string()),
        ];
        let mut terminal = TerminalDriver::spawn_with_no_color(
            Path::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness")),
            arguments,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            no_color,
        )?;
        terminal.resize(36, 120)?;
        terminal.wait_for_text("Quota ·", SEMANTIC_WAIT)?;
        // Owner design 71efc57bb / 873f62923 / a12f9c2db: interactive weekly
        // percentage and reset label occupy two lines; the 5h fact lives in Details.
        terminal.wait_for_styled_text_after("92%", 0, SEMANTIC_WAIT)?;
        terminal.wait_for_styled_text_after("Weekly ·", 0, SEMANTIC_WAIT)?;
        assert_alpha_weekly_main_window(&mut terminal, &fixture, no_color)?;

        // Act: the same real modal keys must work under each fresh color policy.
        let pool_start = terminal.transcript_len();
        terminal.send(b"\x1b[15~")?;
        terminal.wait_for_text_after("Pool details", pool_start, SEMANTIC_WAIT)?;
        let browse_start = terminal.transcript_len();
        terminal.send(b"\x1b")?;
        terminal.wait_for_text_after("Quota ·", browse_start, SEMANTIC_WAIT)?;
        let details_start = terminal.transcript_len();
        terminal.send(b"\r")?;
        terminal.wait_for_text_after("Account details", details_start, SEMANTIC_WAIT)?;
        terminal.wait_for_styled_text_after("OpenAI Alpha", details_start, SEMANTIC_WAIT)?;
        // Preserve the original five-hour percentage/window check at its new home.
        terminal.wait_for_styled_text_after(
            "5h █████████░ 83% left",
            details_start,
            SEMANTIC_WAIT,
        )?;
        let close_start = terminal.transcript_len();
        terminal.send(b"\x1b")?;
        terminal.wait_for_text_after("Quota ·", close_start, SEMANTIC_WAIT)?;
        terminal.send(b"q")?;
        let transcript = terminal.finish(SEMANTIC_WAIT)?;

        // Assert: a color-free child is useful only with its colored positive control.
        let text = String::from_utf8_lossy(&transcript);
        let has_color = text.contains("\x1b[38;") || text.contains("\x1b[48;");
        ensure(
            has_color != no_color,
            "Quota fresh child color policy mismatch",
        )?;
        ensure(
            provider.finish()?.is_empty(),
            "color-policy journey made provider requests",
        )?;
        fixture.assert_read_only()?;
        assert_forbidden_terminal_canaries_absent(&text)?;
        ensure(
            !text.contains("panicked at"),
            "Quota color-policy child panicked",
        )?;
        ensure(
            text.contains("\x1b[?1006l") && text.contains("\x1b[?1049l"),
            "Quota color policy changed terminal teardown",
        )?;
        assert_no_printable_output_after_terminal_restoration(&transcript)?;
    }
    Ok(())
}

/// Preserve the original window attribution across the owner's two-line layout.
fn assert_alpha_weekly_main_window(
    terminal: &mut TerminalDriver,
    fixture: &QuotaResetFixture,
    no_color: bool,
) -> TestResult<()> {
    let prefix = fixture.root().join("weekly-main-window");
    fs::write(
        prefix.with_extension("ansi"),
        terminal.transcript_snapshot(),
    )?;
    fs::write(
        prefix.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({
            "initialColumns":120,"initialRows":36,"resizes":[]
        }))?,
    )?;
    // Reuse the same project libghostty observer as the existing floor-pointer proof.
    // This scenario's initial PTY and sole resize are both exactly120x36.
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/verification/tui/capture/render-pty-failure-grid.ts");
    let output = Command::new("node").arg(script).arg(&prefix).output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "weekly window grid observer failed: {}",
            String::from_utf8_lossy(&output.stderr),
        ))
        .into());
    }
    let text = fs::read_to_string(prefix.with_extension("txt"))?;
    if let Some(directory) = std::env::var_os("CODEX_ROUTER_PTY_CAPTURE_DIR") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory)?;
        for extension in ["ansi", "json", "txt", "grid.json"] {
            fs::copy(
                prefix.with_extension(extension),
                directory.join(format!("no-color-{no_color}-weekly-main.{extension}")),
            )?;
        }
    }
    let rows = text.lines().collect::<Vec<_>>();
    let alpha = rows
        .iter()
        .position(|row| row.contains("OpenAI Alpha"))
        .ok_or("actual quota grid omitted Alpha")?;
    ensure(
        rows.get(alpha).is_some_and(|row| row.contains("92%")),
        "Alpha's main quota row must retain92%",
    )?;
    ensure(
        rows.get(alpha + 1)
            .is_some_and(|row| row.contains("Weekly ·")),
        "Alpha's second quota line must identify its weekly window",
    )?;
    ensure(
        rows.get(alpha..rows.len().min(alpha + 3))
            .ok_or("actual quota grid omitted Alpha window rows")?
            .iter()
            .all(|row| !row.split_whitespace().any(|word| word == "5h") && !row.contains("83%")),
        "Alpha's known5h window belongs in Details, not its weekly main row",
    )?;
    println!("quota no_color={no_color} Alpha92%/Weekly two-line grid verified");
    Ok(())
}
