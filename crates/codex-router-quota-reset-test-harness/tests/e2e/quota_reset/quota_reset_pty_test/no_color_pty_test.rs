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
        terminal.wait_for_text("Sessions", SEMANTIC_WAIT)?;
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
        terminal.wait_for_styled_text_after("92% · week", 0, SEMANTIC_WAIT)?;
        terminal.wait_for_styled_text_after("83% · 5h", 0, SEMANTIC_WAIT)?;

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
