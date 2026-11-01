//! Default-CI real process proof; no browser tools or live providers are needed.
use super::isolated_fixture_test::ProviderFilterFixtureScenario;
use super::*;

#[tokio::test(flavor = "current_thread")]
async fn compiled_quota_legacy_control_slash_drives_help_and_restores_terminal() -> TestResult<()> {
    let fixture =
        QuotaResetFixture::create_provider_filters(ProviderFilterFixtureScenario::MixedAccounts)
            .await?;
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
    terminal
        .wait_for_text("Quota ·", SEMANTIC_WAIT)
        .map_err(|error| {
            std::io::Error::other(format!(
                "quota Browse was not reached before legacy input: {error}; {}",
                terminal.safe_semantic_diagnostics(0),
            ))
        })?;
    let help_start = terminal.transcript_len();
    terminal.send(&[0x1f])?;
    terminal
        .wait_for_text_after("Help", help_start, SEMANTIC_WAIT)
        .map_err(|error| {
            std::io::Error::other(format!(
                "legacy 0x1f did not open Help after Browse was observed and the byte was sent: {error}; {}",
                terminal.safe_semantic_diagnostics(help_start),
            ))
        })?;
    terminal.wait_for_text_after("Actions", help_start, SEMANTIC_WAIT)?;
    // Background filter and activation input must remain owned by Help.
    terminal.send(b"\x06\x1b[B\r")?;
    let close_start = terminal.transcript_len();
    terminal.send(&[0x1f])?;
    terminal.wait_for_text_after("Quota ·", close_start, SEMANTIC_WAIT)?;
    terminal.send(b"q")?;
    let transcript = terminal.finish(SEMANTIC_WAIT)?;
    ensure(
        provider.finish()?.is_empty(),
        "Help-only PTY made provider requests",
    )?;
    fixture.assert_read_only()?;
    assert_no_printable_output_after_terminal_restoration(&transcript)?;
    let text = String::from_utf8_lossy(&transcript);
    ensure(
        !text.contains("Account provider"),
        "Help allowed a background filter action",
    )?;
    ensure(
        text.contains("\u{1b}[?1049h") && text.contains("\u{1b}[?1049l"),
        "Help must restore alternate screen",
    )?;
    assert_forbidden_terminal_canaries_absent(&text)?;
    ensure(!text.contains("panicked at"), "Help PTY child panicked")?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_quota_filters_real_stored_providers_and_disabled_intersections() -> TestResult<()>
{
    for (scenario_index, (scenario, count, result)) in [
        (
            ProviderFilterFixtureScenario::MixedAccounts,
            "Quota · 1/4",
            "Claude Gamma",
        ),
        (
            ProviderFilterFixtureScenario::ClaudeAccountsDisabled,
            "Quota · 0/4",
            "No accounts match these filters",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = QuotaResetFixture::create_provider_filters(scenario).await?;
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
        terminal.wait_for_text("Quota · 4/4", SEMANTIC_WAIT)?;
        let chooser_start = terminal.transcript_len();
        terminal.send(&[0x06])?;
        terminal.wait_for_text_after("Account provider", chooser_start, SEMANTIC_WAIT)?;
        // Design a3439a93 R19 adds Group by between Disabled accounts and Apply.
        terminal.send(b"\r\x1b[B\x1b[B\r\t\t\t")?;
        let apply_start = terminal.transcript_len();
        terminal.send(b"\r")?;
        terminal.wait_for_text_after("Filters: Claude ▾", apply_start, SEMANTIC_WAIT)?;
        terminal.wait_for_text_after("Quota · 2/4", apply_start, SEMANTIC_WAIT)?;
        let toggle_start = terminal.transcript_len();
        terminal.send(&[0x06])?;
        terminal.wait_for_text_after("Account provider", toggle_start, SEMANTIC_WAIT)?;
        let disabled_popup_start = terminal.transcript_len();
        terminal.send(b"\t\r")?;
        terminal.wait_for_text_after("Filter:", disabled_popup_start, SEMANTIC_WAIT)?;
        let disabled_pick_start = terminal.transcript_len();
        terminal.send(b"\x1b[B\r")?;
        terminal.wait_for_styled_text_after("Hidden ▾", disabled_pick_start, SEMANTIC_WAIT)?;
        // Design a3439a93 R19: pass Group by before the unchanged Apply action.
        terminal.send(b"\t\t")?;
        let intersection_start = terminal.transcript_len();
        terminal.send(b"\r")?;
        if let Err(error) = terminal.wait_for_text_after(count, intersection_start, SEMANTIC_WAIT) {
            if let Some(directory) = std::env::var_os("CODEX_ROUTER_PTY_CAPTURE_DIR") {
                let directory = PathBuf::from(directory);
                fs::create_dir_all(&directory)?;
                fs::write(
                    directory.join(format!(
                        "provider-filter-intersection-{scenario_index}.ansi"
                    )),
                    terminal.transcript_snapshot(),
                )?;
            }
            return Err(error);
        }
        terminal.wait_for_text_after(result, intersection_start, SEMANTIC_WAIT)?;
        // In the empty state the unavailable Details reason can displace the
        // long filter caption. Read the committed values in their real form.
        let committed_filter_start = terminal.transcript_len();
        terminal.send(&[0x06])?;
        terminal.wait_for_text_after("Account provider", committed_filter_start, SEMANTIC_WAIT)?;
        terminal.wait_for_styled_text_after("Claude ▾", committed_filter_start, SEMANTIC_WAIT)?;
        terminal.wait_for_styled_text_after("Hidden ▾", committed_filter_start, SEMANTIC_WAIT)?;
        let committed_filter_close_start = terminal.transcript_len();
        terminal.send(b"\x1b")?;
        terminal.wait_for_text_after("Quota ·", committed_filter_close_start, SEMANTIC_WAIT)?;
        terminal.send(b"q")?;
        let transcript = terminal.finish(SEMANTIC_WAIT)?;
        ensure(
            provider.finish()?.is_empty(),
            "filter-only PTY made provider requests",
        )?;
        fixture.assert_read_only()?;
        assert_no_printable_output_after_terminal_restoration(&transcript)?;
        let text = String::from_utf8_lossy(&transcript);
        assert_forbidden_terminal_canaries_absent(&text)?;
        ensure(!text.contains("panicked at"), "filter PTY child panicked")?;
    }
    Ok(())
}

#[test]
fn compiled_sessions_atomic_paste_searches_literal_id_and_restores_terminal_modes() -> TestResult<()>
{
    const SESSION_ID: &str = "00000000-0000-4000-8000-000000000012";
    for (columns, rows) in [(120, 36), (76, 30), (48, 24)] {
        let arguments = [
            OsString::from("--sessions-picker"),
            OsString::from("--provider-filter-storyboard"),
        ];
        let mut terminal = TerminalDriver::spawn(
            Path::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness")),
            arguments,
            Path::new(env!("CARGO_MANIFEST_DIR")),
        )?;
        terminal.resize(rows, columns)?;
        terminal.wait_for_text("Codex Alpha", SEMANTIC_WAIT)?;
        let paste_start = terminal.transcript_len();
        // Control bytes are data inside one terminal-delivered paste, never commands.
        terminal.send(format!("\x1b[200~\x03\x1b{SESSION_ID}\n\r\x0e\x1b[201~").as_bytes())?;
        terminal.wait_for_text_after("1/6", paste_start, SEMANTIC_WAIT)?;
        terminal.wait_for_text_after("Codex Beta", paste_start, SEMANTIC_WAIT)?;
        ensure(
            terminal.child_is_running()?,
            "paste activated or canceled the picker",
        )?;
        terminal.send(&[0x04])?;
        let transcript = terminal.finish(SEMANTIC_WAIT)?;
        let text = String::from_utf8_lossy(&transcript);
        ensure(
            text.contains("SESSION_PICKER_OUTCOME canceled"),
            "only explicit Ctrl+D may close the picker",
        )?;
        ensure(
            !text.contains("SESSION_PICKER_OUTCOME resume:")
                && !text.contains("SESSION_PICKER_OUTCOME fork:")
                && !text.contains("SESSION_PICKER_OUTCOME start-new"),
            "pasted controls activated a session action",
        )?;
        ensure(
            text.contains("\u{1b}[?2004h") && text.contains("\u{1b}[?2004l"),
            "real renderer must enable and restore bracketed paste",
        )?;
        ensure(
            text.contains("\u{1b}[?1049h") && text.contains("\u{1b}[?1049l"),
            "real renderer must restore the alternate screen",
        )?;
        assert_forbidden_terminal_canaries_absent(&text)?;
        ensure(!text.contains("panicked at"), "paste PTY child panicked")?;
    }
    Ok(())
}
