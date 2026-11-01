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
        terminal.send(b"\r\x1b[B\x1b[B\r\t\t")?;
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
        terminal.send(b"\t")?;
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

#[cfg(debug_assertions)]
#[test]
fn compiled_sessions_settled_unbound_keys_flush_only_their_acknowledged_frames() -> TestResult<()> {
    use std::os::unix::fs::DirBuilderExt;

    // Arrange: the actual picker with its existing dispatch receipt/flush owner.
    // This fixture has no record loader, so the three-second reload task has no
    // source publication to confound the input-bounded observation.
    let directory = std::env::temp_dir().join(format!(
        "sessions-unbound-frame-receipts-{}",
        std::process::id()
    ));
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let result: TestResult<()> = (|| {
        let mut terminal = TerminalDriver::spawn_with_environment(
            Path::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness")),
            ["--sessions-picker", "--provider-filter-storyboard"],
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &[("TUI_SESSIONS_FIXTURE_RECEIPT_DIR", directory.as_os_str())],
        )?;
        terminal.wait_for_text("Codex Alpha", SEMANTIC_WAIT)?;

        // Act: each next F12 is sent only after the previous real flush marker.
        // F12 is unbound; no sleeps, polling interval, or timing threshold.
        for sequence in 1..=8 {
            terminal.send(b"\x1b[24~")?;
            terminal.wait_for_text(
                &format!("\x1b]7770;dispatch={sequence}\x1b\\"),
                SEMANTIC_WAIT,
            )?;
        }
        terminal.send(&[0x04])?;
        let transcript = terminal.finish(SEMANTIC_WAIT)?;
        if let Some(capture) = std::env::var_os("CODEX_ROUTER_PTY_CAPTURE_DIR") {
            let capture = PathBuf::from(capture);
            fs::create_dir_all(&capture)?;
            fs::write(capture.join("picker-unbound-frame-probe.ansi"), &transcript)?;
        }

        // Assert: between consecutive ACKs there is exactly one native frame,
        // the marker producer's intentional flush for the next unbound input.
        let text = String::from_utf8_lossy(&transcript);
        let mut frames_between_acks = Vec::new();
        for sequence in 1..8 {
            let previous = format!("\x1b]7770;dispatch={sequence}\x1b\\");
            let next = format!("\x1b]7770;dispatch={}\x1b\\", sequence + 1);
            let start =
                text.find(&previous).ok_or("previous dispatch ACK absent")? + previous.len();
            let end = text.find(&next).ok_or("next dispatch ACK absent")?;
            let between = text.get(start..end).ok_or("dispatch ACK order changed")?;
            frames_between_acks.push(between.matches("\x1b[?2026l").count());
        }
        println!("picker_unbound_frames_between_acks={frames_between_acks:?}");
        ensure(
            frames_between_acks == vec![1; 7],
            "settled picker painted extra frames between unbound-key ACKs",
        )?;
        let receipts = fs::read_to_string(directory.join("input-dispatch.jsonl"))?
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        ensure(receipts.len() == 9, "unbound probe input count changed")?;
        for (index, receipt) in receipts.iter().take(8).enumerate() {
            ensure(
                receipt["sequence"] == index + 1
                    && receipt["event"]["code"]["name"] == "F12"
                    && receipt["surface"]["kind"] == "screen"
                    && receipt["action"].is_null(),
                "probe key was not an acknowledged unbound screen input",
            )?;
        }
        ensure(
            text.contains("SESSION_PICKER_OUTCOME canceled")
                && text.contains("\x1b[?1006l")
                && text.contains("\x1b[?1049l"),
            "frame probe changed cancellation or terminal restoration",
        )?;
        Ok(())
    })();
    for name in ["input-dispatch.jsonl", "picker-outcome.json"] {
        let path = directory.join(name);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    fs::remove_dir(directory)?;
    result
}
