#[tokio::test(flavor = "current_thread")]
async fn compiled_credit_options_refresh_and_save_use_only_the_sealed_loopback_provider()
-> TestResult<()> {
    let fixture = QuotaResetFixture::create().await?;
    let focused_account_id = AccountId::new("acct_pty_alpha")?;
    let initial_state =
        AsyncSqliteStateStore::open_read_only(&fixture.root().join("state.sqlite")).await?;
    ensure(
        !initial_state
            .load_account_credit_usage_policy(&focused_account_id)
            .await?
            .allows_credit_usage(),
        "focused policy should start at Disallow",
    )?;
    initial_state.close().await?;
    let mut provider = HeldLoopbackProvider::bind()?;
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

    stage(
        terminal.wait_for_text("Filters", SEMANTIC_WAIT),
        "account-options initial browse",
    )?;
    let inspection_start = terminal.transcript_len();
    terminal.send(&[0x12])?;
    stage(
        provider.wait_for_request_count(2, SEMANTIC_WAIT),
        "reset inspection loopback requests",
    )?;
    stage(
        terminal.wait_for_text_after("Reset credits", inspection_start, SEMANTIC_WAIT),
        "Resets tab opens for the focused account",
    )?;

    let credits_tab_start = terminal.transcript_len();
    terminal.send(b"\t\x1b[C\r")?;
    stage(
        terminal.wait_for_text_after("Options · Credits", credits_tab_start, SEMANTIC_WAIT),
        "inspection-only cancellation acknowledgement before Credits",
    )?;
    provider.release_get_responses()?;
    let refresh_start = terminal.transcript_len();
    terminal.send(b"r")?;
    stage(
        provider.wait_for_request_count(10, SEMANTIC_WAIT),
        "explicit quota and credit refresh loopback requests",
    )?;
    stage(
        terminal.wait_for_text_after("Credit balance refreshed", refresh_start, SEMANTIC_WAIT),
        "explicit refresh completion",
    )?;
    terminal.send(b"\x1b[H")?;
    stage(
        terminal.wait_for_text_after("Available · 42", refresh_start, SEMANTIC_WAIT),
        "loopback provider balance display",
    )?;

    let first_edit_start = terminal.transcript_len();
    terminal.send(b"\x1b[13;5u")?;
    stage(
        terminal.wait_for_styled_text_after("Disallow ▾", first_edit_start, SEMANTIC_WAIT),
        "credit policy editing state",
    )?;
    let first_allow_start = terminal.transcript_len();
    terminal.send(b"\r")?;
    terminal.wait_for_text_after("Filter:", first_allow_start, SEMANTIC_WAIT)?;
    let first_allow_start = terminal.transcript_len();
    terminal.send(b"\x1b[B")?;
    stage(
        terminal.wait_for_text_after("Allow", first_allow_start, SEMANTIC_WAIT),
        "credit policy draft selection",
    )?;
    let first_choice_selected_start = terminal.transcript_len();
    terminal.send(b"\r")?;
    terminal.wait_for_styled_text_after("Allow ▾", first_choice_selected_start, SEMANTIC_WAIT)?;
    stage(
        terminal.wait_for_text_after("Save", first_choice_selected_start, SEMANTIC_WAIT),
        "selected Allow draft collapses before cancellation",
    )?;
    let cancel_draft_start = terminal.transcript_len();
    terminal.send(b"\x1b")?;
    stage(
        terminal.wait_for_text_after("Edit", cancel_draft_start, SEMANTIC_WAIT),
        "cancelled credit policy draft returns to saved Credits view",
    )?;
    let cancelled_state =
        AsyncSqliteStateStore::open_read_only(&fixture.root().join("state.sqlite")).await?;
    ensure(
        !cancelled_state
            .load_account_credit_usage_policy(&focused_account_id)
            .await?
            .allows_credit_usage(),
        "Esc from an Allow draft must leave native SQLite at Disallow",
    )?;
    cancelled_state.close().await?;

    let second_edit_start = terminal.transcript_len();
    terminal.send(b"\x1b[13;5u")?;
    stage(
        terminal.wait_for_styled_text_after("Disallow ▾", second_edit_start, SEMANTIC_WAIT),
        "second policy edit starts from the saved Disallow value",
    )?;
    let second_allow_start = terminal.transcript_len();
    terminal.send(b"\r")?;
    terminal.wait_for_text_after("Filter:", second_allow_start, SEMANTIC_WAIT)?;
    let second_allow_start = terminal.transcript_len();
    terminal.send(b"\x1b[B")?;
    stage(
        terminal.wait_for_text_after("Allow", second_allow_start, SEMANTIC_WAIT),
        "second Allow draft selection",
    )?;
    let second_choice_selected_start = terminal.transcript_len();
    terminal.send(b"\r")?;
    terminal.wait_for_styled_text_after("Allow ▾", second_choice_selected_start, SEMANTIC_WAIT)?;
    stage(
        terminal.wait_for_text_after("Save", second_choice_selected_start, SEMANTIC_WAIT),
        "selected Allow draft collapses before save",
    )?;
    let saved_policy_start = terminal.transcript_len();
    terminal.send(b"\x1b[13;5u")?;
    if let Err(error) = terminal.wait_for_text_after("Edit", saved_policy_start, SEMANTIC_WAIT) {
        let diagnostics = terminal.safe_semantic_diagnostics(saved_policy_start);
        return Err(std::io::Error::other(format!(
            "saved credit policy readback in the pane: {error}; {diagnostics}"
        ))
        .into());
    }
    let saved_state =
        AsyncSqliteStateStore::open_read_only(&fixture.root().join("state.sqlite")).await?;
    ensure(
        saved_state
            .load_account_credit_usage_policy(&focused_account_id)
            .await?
            .allows_credit_usage(),
        "explicit save should persist Allow in native SQLite",
    )?;
    saved_state.close().await?;
    let browse_start = terminal.transcript_len();
    terminal.send(b"\x1b")?;
    if let Err(error) = terminal.wait_for_text_after("Filters", browse_start, SEMANTIC_WAIT) {
        let diagnostics = terminal.safe_semantic_diagnostics(browse_start);
        return Err(std::io::Error::other(format!(
            "account-options returns to browse: {error}; {diagnostics}"
        ))
        .into());
    }

    let reopen_resets_start = terminal.transcript_len();
    terminal.send(&[0x12])?;
    stage(
        terminal.wait_for_text_after("Reset credits", reopen_resets_start, SEMANTIC_WAIT),
        "reopened Resets pane before saved policy reload",
    )?;
    stage(
        provider.wait_for_request_count(12, SEMANTIC_WAIT),
        "reopened reset inspection loopback requests",
    )?;
    let reopen_credits_start = terminal.transcript_len();
    terminal.send(b"\t\x1b[C\r")?;
    stage(
        terminal.wait_for_text_after("Options · Credits", reopen_credits_start, SEMANTIC_WAIT),
        "reopened Credits tab reads saved policy",
    )?;
    let reopen_editor_start = terminal.transcript_len();
    terminal.send(b"\x1b[13;5u")?;
    stage(
        terminal.wait_for_styled_text_after("Allow ▾", reopen_editor_start, SEMANTIC_WAIT),
        "reopened policy editor displays the saved Allow value",
    )?;
    let reopened_popup_start = terminal.transcript_len();
    terminal.send(b"\r")?;
    terminal.wait_for_text_after("Filter:", reopened_popup_start, SEMANTIC_WAIT)?;
    let reopened_choice_cancel_start = terminal.transcript_len();
    terminal.send(b"\x1b")?;
    stage(
        terminal.wait_for_text_after("Save", reopened_choice_cancel_start, SEMANTIC_WAIT),
        "reopened inline choice collapses before cancelling the editor",
    )?;
    let reopen_cancel_start = terminal.transcript_len();
    terminal.send(b"\x1b")?;
    stage(
        terminal.wait_for_text_after("Edit", reopen_cancel_start, SEMANTIC_WAIT),
        "reopened policy editor cancels back to Credits",
    )?;
    let final_browse_start = terminal.transcript_len();
    terminal.send(b"\x1b")?;
    stage(
        terminal.wait_for_text_after("Filters", final_browse_start, SEMANTIC_WAIT),
        "reopened account-options pane closes to Browse",
    )?;
    terminal.send(b"q")?;
    let transcript = stage(terminal.finish(SEMANTIC_WAIT), "credit options child exit")?;
    let requests = stage(provider.finish(), "credit provider shutdown")?;

    ensure(
        requests.len() == 12 && requests.iter().all(|request| request.method == "GET"),
        "credit refresh and inspection reopen must use twelve loopback GETs and no consume POST",
    )?;
    fixture.assert_secrets_unchanged()?;
    let state = AsyncSqliteStateStore::open_read_only(&fixture.root().join("state.sqlite")).await?;
    let account_ids = [
        AccountId::new("acct_pty_alpha")?,
        AccountId::new("acct_pty_beta")?,
    ];
    let mut allowed_accounts = 0;
    let mut observed_credit_balance = false;
    for account_id in &account_ids {
        let policy = state.load_account_credit_usage_policy(account_id).await?;
        if policy.allows_credit_usage() {
            allowed_accounts += 1;
        }
        if let Some(observation) = state.load_account_credit_observation(account_id).await? {
            observed_credit_balance |= observation.provider_observation().availability()
                == &CreditAvailability::Available {
                    balance: Some(
                        CreditBalance::new("42.00").expect("fixture credit balance is valid"),
                    ),
                };
        }
    }
    ensure(
        allowed_accounts == 1,
        "one focused account policy must be Allow",
    )?;
    ensure(
        state
            .load_account_credit_usage_policy(&focused_account_id)
            .await?
            .allows_credit_usage(),
        "reopened focused account policy should remain Allow",
    )?;
    ensure(
        observed_credit_balance,
        "loopback credit observation was not persisted",
    )?;
    state.close().await?;

    let transcript_text = String::from_utf8_lossy(&transcript);
    assert_forbidden_terminal_canaries_absent(&transcript_text)?;
    ensure(
        !transcript_text.contains("chatgpt.com") && !transcript_text.contains("api.openai.com"),
        "credit refresh transcript mentioned a production origin",
    )?;
    ensure(
        !transcript_text.contains("panicked at"),
        "credit options transcript contained a child panic",
    )?;
    if let Some(capture_directory) = std::env::var_os("CODEX_ROUTER_PTY_CAPTURE_DIR") {
        let capture_directory = PathBuf::from(capture_directory);
        fs::create_dir_all(&capture_directory)?;
        fs::write(
            capture_directory.join("compiled-credit-options-loopback-120x36.ansi"),
            &transcript,
        )?;
        fs::write(
            capture_directory.join("compiled-credit-options-loopback-120x36.meta"),
            "viewport=120x36\ndata=synthetic balance from sealed loopback provider\njourney=compiled CLI refreshed Available 42.00 and saved Allow to SQLite\nproduction_calls=none\n",
        )?;
    }
    Ok(())
}
