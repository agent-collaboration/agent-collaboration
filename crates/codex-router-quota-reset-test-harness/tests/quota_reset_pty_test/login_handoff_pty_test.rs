//! The real CLI tears down quota before a recording child inherits its PTY.
use super::{
    SEMANTIC_WAIT,
    isolated_fixture_test::{QuotaResetFixture, TestResult, ensure},
    loopback_provider_test::HeldLoopbackProvider,
    terminal_interaction_test::TerminalDriver,
};
use portable_pty::CommandBuilder;
use serde::Deserialize;
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LoginHandoffReceipt {
    arguments: Vec<String>,
    terminal: LoginTerminalReceipt,
    exit_code: u8,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum LoginTerminalReceipt {
    NotTerminal,
    Interactive {
        canonical: bool,
        echo: bool,
        signals: bool,
        foreground: bool,
    },
}

#[tokio::test(flavor = "current_thread")]
async fn real_login_child_inherits_restored_terminal_and_returns_after_each_exit_status()
-> TestResult<()> {
    let evidence = std::env::var_os("QUOTA_LOGIN_PTY_EVIDENCE_DIR").map(PathBuf::from);
    if let Some(directory) = &evidence {
        fs::create_dir(directory)?;
    }
    for (exit_code, notice) in [
        (0u8, "Logged in again: pty-beta"),
        (130, "Login cancelled: pty-beta"),
        (7, "Login failed for pty-beta"),
    ] {
        let fixture = QuotaResetFixture::create().await?;
        let provider = HeldLoopbackProvider::bind()?;
        let binaries = fixture.root().join("login-bin");
        fs::create_dir(&binaries)?;
        fs::set_permissions(&binaries, fs::Permissions::from_mode(0o700))?;
        symlink(
            env!("CARGO_BIN_EXE_quota-login-recording-standin"),
            binaries.join("codex-router"),
        )?;
        let receipt_file = fixture.root().join("login-receipt.jsonl");
        let mut command =
            CommandBuilder::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness"));
        command.args([
            OsString::from("--router-root"),
            fixture.root().as_os_str().to_owned(),
            OsString::from("--fixture-capability"),
            OsString::from(fixture.capability()),
            OsString::from("--provider-listener"),
            OsString::from(provider.address().to_string()),
        ]);
        command.env_clear();
        command.env("HOME", std::env::temp_dir());
        command.env("TMPDIR", std::env::temp_dir());
        command.env("TERM", "xterm-256color");
        command.env("LANG", "C.UTF-8");
        command.env("PATH", &binaries);
        command.env("QUOTA_LOGIN_STANDIN_RECEIPT", &receipt_file);
        command.env("QUOTA_LOGIN_STANDIN_EXIT", exit_code.to_string());
        command.cwd(Path::new(env!("CARGO_MANIFEST_DIR")));
        let mut terminal = TerminalDriver::spawn_prepared_command(command, None)?;
        terminal.wait_for_text("Quota ·", SEMANTIC_WAIT)?;
        terminal.send(b"\x1b[B")?;
        let menu_start = terminal.transcript_len();
        terminal.send(b"\x1b[21~")?;
        terminal.wait_for_text_after("Log in again", menu_start, SEMANTIC_WAIT)?;
        for _ in 0..5 {
            terminal.send(b"\x1b[B")?;
        }
        let login_start = terminal.transcript_len();
        terminal.send(b"\r")?;
        terminal.wait_for_text_after(notice, login_start, SEMANTIC_WAIT)?;
        ensure(
            terminal.child_is_running()?,
            "quota must restart in the same retained CLI child",
        )?;
        terminal.send(b"q")?;
        let transcript = terminal.finish(SEMANTIC_WAIT)?;
        let bytes = fs::read(&receipt_file)?;
        let receipts = String::from_utf8(bytes.clone())?
            .lines()
            .map(serde_json::from_str::<LoginHandoffReceipt>)
            .collect::<Result<Vec<_>, _>>()?;
        ensure(
            receipts.len() == 1,
            "one login outcome must launch exactly one recording child",
        )?;
        let receipt = receipts.first().ok_or("login child receipt missing")?;
        ensure(
            receipt.exit_code == exit_code,
            "recorded login exit status must match the injected outcome",
        )?;
        ensure(
            receipt.arguments
                == vec![
                    "account",
                    "login",
                    "--provider",
                    "openai",
                    "--label",
                    "pty-beta",
                    "--router-root",
                    fixture
                        .root()
                        .to_str()
                        .ok_or_else(|| std::io::Error::other("fixture root is not UTF-8"))?,
                ],
            "login child argv must match the captured provider, label and router root",
        )?;
        ensure(
            matches!(
                receipt.terminal,
                LoginTerminalReceipt::Interactive {
                    canonical: true,
                    echo: true,
                    signals: true,
                    foreground: true
                }
            ),
            "login must inherit the actual restored foreground terminal",
        )?;
        ensure(
            provider.finish()?.is_empty(),
            "the login recording stand-in must make no provider requests",
        )?;
        fixture.assert_read_only()?;
        if let Some(directory) = &evidence {
            fs::write(
                directory.join(format!("exit-{exit_code}.ansi")),
                &transcript,
            )?;
            fs::write(directory.join(format!("exit-{exit_code}.jsonl")), bytes)?;
        }
    }
    Ok(())
}
