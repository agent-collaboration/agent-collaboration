//! Line protocol around the existing PTY owner. The TypeScript observer owns
//! grid decisions; this bridge never substitutes a rendered screen.
use super::isolated_fixture_test::{
    ProviderFilterFixtureScenario, QuotaResetFixture, TestResult, ensure,
};
use super::loopback_provider_test::HeldLoopbackProvider;
use super::terminal_interaction_test::{KeyboardProtocol, TerminalDriver};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

pub(super) fn emit(line: &str) -> TestResult<()> {
    let mut output = std::io::stdout().lock();
    writeln!(output, "TUI_GRID {line}")?;
    output.flush()?;
    Ok(())
}
fn decode_hex(encoded: &str) -> TestResult<Vec<u8>> {
    ensure(encoded.len().is_multiple_of(2), "odd byte encoding")?;
    encoded
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digits = std::str::from_utf8(pair)?;
            Ok(u8::from_str_radix(digits, 16)?)
        })
        .collect()
}
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(super) async fn run_grid_terminal() -> TestResult<()> {
    let surface = std::env::var("TUI_GRID_SURFACE")?;
    let protocol = match std::env::var("TUI_GRID_KEYS")?.as_str() {
        "legacy" => KeyboardProtocol::Legacy,
        "kitty" => KeyboardProtocol::Kitty,
        _ => return Err("unknown terminal key mode".into()),
    };
    let fixture = if surface == "quota-filters" {
        QuotaResetFixture::create_provider_filters(ProviderFilterFixtureScenario::MixedAccounts)
            .await?
    } else {
        QuotaResetFixture::create().await?
    };
    let provider = HeldLoopbackProvider::bind()?;
    provider.release_get_responses()?;
    let arguments = match surface.as_str() {
        "sessions" => vec![OsString::from("--sessions-picker")],
        "sessions-filters" => vec![
            OsString::from("--sessions-picker"),
            OsString::from("--provider-filter-storyboard"),
            OsString::from("--fixture-capability"),
            OsString::from(fixture.capability()),
        ],
        "quota" | "quota-filters" => vec![
            OsString::from("--router-root"),
            fixture.root().as_os_str().to_owned(),
            OsString::from("--fixture-capability"),
            OsString::from(fixture.capability()),
            OsString::from("--provider-listener"),
            OsString::from(provider.address().to_string()),
        ],
        "normal" | "hard" => vec![
            OsString::from(
                "quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture",
            ),
            OsString::from("--exact"),
            OsString::from("--ignored"),
            OsString::from("--nocapture"),
            OsString::from("--test-threads=1"),
        ],
        _ => return Err("unknown grid capture surface".into()),
    };
    fixture.assert_read_only()?;
    let component_binary = std::env::var_os("TUI_GRID_COMPONENT_BINARY");
    let receipt_path = std::env::var_os("TUI_GRID_COMPONENT_RECEIPT");
    let release_path = std::env::var_os("TUI_GRID_COMPONENT_RELEASE");
    let component = matches!(surface.as_str(), "normal" | "hard");
    let mut environment: Vec<(&str, &OsStr)> = Vec::new();
    let sessions_receipt_directory = std::env::var_os("TUI_SESSIONS_FIXTURE_RECEIPT_DIR");
    if let Some(directory) = sessions_receipt_directory.as_deref() {
        ensure(
            matches!(surface.as_str(), "sessions" | "sessions-filters"),
            "sessions receipt directory requires a sessions fixture surface",
        )?;
        let directory_path = Path::new(directory);
        let metadata = std::fs::symlink_metadata(directory_path)?;
        ensure(
            directory_path.is_absolute()
                && metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && std::fs::read_dir(directory_path)?.next().is_none(),
            "sessions receipt directory must be an empty owned directory",
        )?;
        environment.push(("TUI_SESSIONS_FIXTURE_RECEIPT_DIR", directory));
    }
    let storyboard_variant = std::env::var_os("TUI_GRID_STORYBOARD_VARIANT");
    if let Some(variant) = storyboard_variant.as_deref() {
        ensure(
            valid_storyboard_variant(&surface, variant),
            "invalid grid storyboard variant or surface",
        )?;
        environment.push(("CODEX_ROUTER_TUI_STORYBOARD_VARIANT", variant));
    }
    if component {
        environment.push((
            "QUOTA_COMPONENT_FIXTURE",
            OsStr::new(if surface == "normal" {
                "normal"
            } else {
                "visual-hard-cases"
            }),
        ));
        environment.push((
            "QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE",
            receipt_path
                .as_deref()
                .ok_or("component receipt required")?,
        ));
        if let Some(release) = release_path.as_deref() {
            environment.push(("QUOTA_COMPONENT_FIXTURE_RELEASE_FILE", release));
        }
    }
    let binary = if component {
        component_binary
            .as_deref()
            .ok_or("component binary required")?
    } else {
        OsStr::new(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness"))
    };
    let terminal = TerminalDriver::spawn_with_environment(
        Path::new(binary),
        arguments,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &environment,
    )?;
    let outcome = drive_terminal(terminal, protocol)?;
    if component {
        let receipt = std::fs::read_to_string(
            receipt_path
                .as_deref()
                .ok_or("component receipt required")?,
        )?;
        let expected = if surface == "normal" {
            "scenario=Normal"
        } else {
            "scenario=VisualHardCases"
        };
        ensure(
            receipt.contains(expected)
                && receipt.contains("external_requests=0 persisted_writes=0"),
            "component fixture receipt mismatch",
        )?;
    }
    fixture.assert_read_only()?;
    let records = provider.finish()?;
    ensure(
        records.iter().all(|record| record.method == "GET"),
        "read-only journey sent a write",
    )?;
    emit(&format!(
        "{} {}",
        if outcome.completed {
            "FINISHED"
        } else {
            "STOPPED"
        },
        outcome.bytes
    ))?;
    Ok(())
}

fn valid_storyboard_variant(surface: &str, variant: &OsStr) -> bool {
    surface == "sessions-filters"
        && [
            "wide-glyph-left-border",
            "source-qualified-outcome",
            "configured-source-new",
        ]
        .iter()
        .any(|candidate| variant == OsStr::new(candidate))
}

#[test]
fn storyboard_variants_are_explicit_and_sessions_only() {
    for variant in [
        "wide-glyph-left-border",
        "source-qualified-outcome",
        "configured-source-new",
    ] {
        assert!(valid_storyboard_variant(
            "sessions-filters",
            OsStr::new(variant)
        ));
        assert!(!valid_storyboard_variant("quota", OsStr::new(variant)));
        assert!(!valid_storyboard_variant("sessions", OsStr::new(variant)));
    }
    assert!(!valid_storyboard_variant(
        "sessions-filters",
        OsStr::new("unknown")
    ));
}

pub(super) struct TerminalOutcome {
    pub(super) completed: bool,
    pub(super) bytes: usize,
}
pub(super) fn drive_terminal(
    mut terminal: TerminalDriver,
    protocol: KeyboardProtocol,
) -> TestResult<TerminalOutcome> {
    emit("READY 120 36")?;
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let mut words = line.split_whitespace();
        match words.next() {
            Some("read") => {
                let milliseconds: u64 = words.next().ok_or("read deadline required")?.parse()?;
                ensure(milliseconds <= 30_000, "read deadline too long")?;
                match terminal.receive_bytes(Duration::from_millis(milliseconds)) {
                    Ok(Some(bytes)) => emit(&format!("BYTES {}", encode_hex(&bytes)))?,
                    Ok(None) => emit("EOF")?,
                    Err(error) => emit(&format!("READ_ERROR {error}"))?,
                }
            }
            Some("write" | "reply") => {
                let bytes = decode_hex(words.next().ok_or("bytes required")?)?;
                if line.starts_with("reply ") {
                    terminal.send_terminal_reply(&bytes, protocol)?;
                } else {
                    // One command is exactly one send/write_all transport burst.
                    terminal.send(&bytes)?;
                }
                emit("OK")?;
            }
            Some("resize") => {
                let cols = words.next().ok_or("columns required")?.parse()?;
                let rows = words.next().ok_or("rows required")?.parse()?;
                terminal.resize(rows, cols)?;
                emit("OK")?;
            }
            Some("running") => emit(if terminal.child_is_running()? {
                "RUNNING"
            } else {
                "EXITED"
            })?,
            Some("finish") => {
                let transcript = terminal.finish(Duration::from_secs(8))?;
                return Ok(TerminalOutcome {
                    completed: true,
                    bytes: transcript.len(),
                });
            }
            Some("stop") => {
                let transcript = terminal.terminate_and_reap(Duration::from_secs(8))?;
                return Ok(TerminalOutcome {
                    completed: false,
                    bytes: transcript.len(),
                });
            }
            _ => return Err("unknown grid driver command".into()),
        }
    }
    Err("grid observer closed before explicit cleanup".into())
}

#[test]
fn sessions_receipt_input_crosses_the_real_pty_environment_boundary() -> TestResult<()> {
    let receipt_directory = Path::new("/fixture-owned/sessions-receipts");
    let terminal = TerminalDriver::spawn_with_environment(
        Path::new("/usr/bin/env"),
        std::iter::empty::<&str>(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &[(
            "TUI_SESSIONS_FIXTURE_RECEIPT_DIR",
            receipt_directory.as_os_str(),
        )],
    )?;
    let transcript = String::from_utf8(terminal.finish(Duration::from_secs(8))?)?;
    ensure(
        transcript.lines().any(|line| {
            line.trim_end() == "TUI_SESSIONS_FIXTURE_RECEIPT_DIR=/fixture-owned/sessions-receipts"
        }),
        "nominated sessions receipt directory did not reach the PTY child",
    )?;
    let terminal = TerminalDriver::spawn_with_environment(
        Path::new("/usr/bin/env"),
        std::iter::empty::<&str>(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &[],
    )?;
    let transcript = String::from_utf8(terminal.finish(Duration::from_secs(8))?)?;
    ensure(
        !transcript.contains("TUI_SESSIONS_FIXTURE_RECEIPT_DIR="),
        "sessions receipt input acquired an ambient fallback",
    )
}

#[test]
fn sessions_wrapper_receipt_input_reaches_its_child_on_the_existing_pty_owner() -> TestResult<()> {
    use crate::native_adapter_fixture_support::NativeFixture;
    use portable_pty::CommandBuilder;
    for (surface, nominated) in [("sessions", true), ("sessions", false), ("quota", true)] {
        let fixture = NativeFixture::prepare(surface)?;
        let output = fixture.root.join("output");
        let directory = output.join("sessions-receipts");
        std::fs::create_dir(&directory)?;
        let configuration = fixture.root.join("configuration.json");
        let wrapper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/verification/tui/fixture-child.ts");
        let mut command = CommandBuilder::new(&fixture.node);
        command.args([wrapper.as_os_str(), configuration.as_os_str()]);
        command.env_clear();
        command.env("QUOTA_STORYBOARD_FIXTURE_COMMAND", &configuration);
        if nominated {
            command.env("TUI_SESSIONS_FIXTURE_RECEIPT_DIR", &directory);
        }
        command.cwd(&fixture.root);
        let terminal = TerminalDriver::spawn_prepared_command(command, None)?;
        terminal.finish(Duration::from_secs(8))?;
        let environment_keys = fixture.read("environment-keys.txt")?;
        ensure(
            environment_keys
                .lines()
                .any(|key| key == "TUI_SESSIONS_FIXTURE_RECEIPT_DIR")
                == (surface == "sessions" && nominated),
            "sessions wrapper omitted the nomination or forwarded it outside sessions",
        )?;
        ensure(
            std::fs::read_dir(&directory)?.next().is_none(),
            "transport wrote receipts in place of the product producer",
        )?;
        fixture.cleanup()?;
    }
    Ok(())
}
