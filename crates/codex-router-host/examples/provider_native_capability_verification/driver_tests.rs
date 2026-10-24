//! The verifier's own drivers: process ownership by real UID, bounded process output,
//! option parsing and the PTY driver.
//!
//! Terminal cases drive only `/bin/cat`, a harmless local subprocess these tests start
//! and stop; they prove the driver, never client or product behavior.
use crate::bounded_process::run_bounded;
use crate::command_options::{VerifierCommand, parse_command};
use crate::installed_client_terminal::{
    InstalledClientTerminal, contains_word, shows_trust_dialog, terminal_text,
};
use crate::journey_run::name_fragment;
use crate::native_journeys::SelectionCase;
use crate::process_identity::{
    TABLE_FIELDS, own_real_uid, owned_closure, parse_process_table, remaining_owned,
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;

fn table_row(pid: u32, parent: u32, real_uid: u32, command: &str) -> String {
    format!("{pid} {parent} {real_uid} {pid} Sat Oct 24 10:00:00 2026 {command}")
}

#[test]
fn ownership_is_the_real_uid_column() {
    // ps(1): `ruid` is the real user ID; `uid` is effective and a setuid child raises it.
    assert!(TABLE_FIELDS.split(',').any(|field| field == "ruid="));
    assert!(!TABLE_FIELDS.split(',').any(|field| field == "uid="));
    let own = own_real_uid().unwrap();
    let id = Command::new("/usr/bin/id").arg("-ru").output().unwrap();
    assert_eq!(own.to_string(), String::from_utf8_lossy(&id.stdout).trim());
}

#[test]
fn process_rows_parse_with_spaced_commands_and_malformed_rows_fail() {
    let table = parse_process_table(&format!(
        "{}\n\n",
        table_row(10, 1, 501, "/Applications/Some App/bin")
    ))
    .unwrap();
    assert_eq!(
        table[&10].executable,
        PathBuf::from("/Applications/Some App/bin")
    );
    assert_eq!(table[&10].birth, "Sat Oct 24 10:00:00 2026");
    // macOS ps prints the uid_t of `nobody` as -2.
    let nobody =
        parse_process_table("8473 1 -2 8473 Sun 18 Oct 06:08:30 2026 /usr/libexec/dhcp6d\n")
            .unwrap();
    assert_eq!(nobody[&8473].real_uid, 4_294_967_294);
    assert!(parse_process_table("10 1 501 10 Sat Oct\n").is_err());
    assert!(parse_process_table("ten 1 501 10 Sat Oct 24 10:00:00 2026 /bin\n").is_err());
}

#[test]
fn owned_closure_follows_real_owner_and_never_acquires_foreign_or_reused_processes() {
    let observed = parse_process_table(
        &[
            table_row(10, 1, 501, "/candidate/codex-router"),
            // A setuid descendant keeps its real owner: it is owned.
            table_row(11, 10, 501, "/usr/bin/top"),
            table_row(12, 11, 501, "/bin/sh"),
            table_row(20, 1, 501, "/unrelated"),
        ]
        .join("\n"),
    )
    .unwrap();
    let owned = owned_closure(10, 501, &BTreeMap::new(), &observed).unwrap();
    assert_eq!(owned.keys().copied().collect::<Vec<_>>(), [10, 11, 12]);
    let foreign = parse_process_table(
        &[
            table_row(10, 1, 501, "/candidate"),
            table_row(11, 10, 0, "/root-owned"),
        ]
        .join("\n"),
    )
    .unwrap();
    assert!(
        owned_closure(10, 501, &BTreeMap::new(), &foreign)
            .unwrap_err()
            .contains("foreign real owner")
    );
    // PID 11 now names a different process: neither still owned nor acquired.
    let reused = parse_process_table(
        &[
            table_row(10, 1, 501, "/candidate/codex-router"),
            table_row(11, 10, 501, "/other"),
        ]
        .join("\n"),
    )
    .unwrap();
    assert_eq!(
        remaining_owned(&owned, &reused)
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [10]
    );
    let reacquired = owned_closure(10, 501, &owned, &reused).unwrap();
    assert!(!reacquired.contains_key(&11));
}

#[tokio::test(flavor = "multi_thread")]
async fn process_output_is_bounded_while_reading() {
    let mut fits = tokio::process::Command::new("/bin/echo");
    fits.arg("bounded");
    let output = run_bounded(fits, "echo", Duration::from_secs(10), 64, 64)
        .await
        .unwrap();
    assert_eq!(output.stdout, b"bounded\n");
    let mut flood = tokio::process::Command::new("/usr/bin/yes");
    flood.arg("overflow");
    let error = run_bounded(flood, "yes", Duration::from_secs(10), 1024, 64)
        .await
        .err()
        .unwrap();
    assert!(error.contains("bound"), "{error}");
    let mut slow = tokio::process::Command::new("/bin/sleep");
    slow.arg("30");
    let error = run_bounded(slow, "sleep", Duration::from_millis(300), 64, 64)
        .await
        .err()
        .unwrap();
    assert!(error.contains("did not finish"), "{error}");
}

fn words(text: &[&str]) -> Vec<String> {
    text.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn command_options_are_strict() {
    assert_eq!(
        parse_command(&words(&["--help"])).unwrap(),
        VerifierCommand::Help
    );
    assert_eq!(
        parse_command(&words(&[
            "select",
            "plan.json",
            "--case",
            "two-skills",
            "--arguments",
            " a b ",
            "--output",
            "out"
        ]))
        .unwrap(),
        VerifierCommand::Select {
            plan: "plan.json".into(),
            output: "out".into(),
            case: SelectionCase::TwoSkills,
            arguments: " a b ".to_owned(),
        }
    );
    assert_eq!(
        parse_command(&words(&[
            "processes",
            "capture",
            "--root-pid",
            "42",
            "--output",
            "inventory.json"
        ]))
        .unwrap(),
        VerifierCommand::CaptureProcesses {
            root_pid: 42,
            output: "inventory.json".into()
        }
    );
    assert_eq!(
        parse_command(&words(&[
            "commands",
            "plan.json",
            "--arguments",
            "x",
            "--output",
            "out",
            "--reload-probe"
        ]))
        .unwrap(),
        VerifierCommand::Commands {
            plan: "plan.json".into(),
            output: "out".into(),
            arguments: "x".to_owned(),
            reload_probe: true,
        }
    );
    let wrong = [
        vec![],
        words(&["catalogue", "--output", "out"]),
        words(&["catalogue", "a.json", "b.json", "--output", "out"]),
        words(&[
            "catalogue",
            "plan.json",
            "--output",
            "out",
            "--output",
            "again",
        ]),
        words(&["catalogue", "plan.json", "--output", "out", "--unknown"]),
        words(&[
            "select",
            "plan.json",
            "--case",
            "sideways",
            "--arguments",
            "x",
            "--output",
            "out",
        ]),
        words(&[
            "history",
            "plan.json",
            "--arguments",
            "x",
            "--output",
            "out",
        ]),
        words(&["processes", "check", "--root-pid", "1"]),
        words(&["processes", "capture", "--root-pid", "pid", "--output", "x"]),
        words(&["install"]),
    ];
    for command in wrong {
        assert!(parse_command(&command).is_err(), "{command:?}");
    }
}

#[test]
fn screen_text_and_word_matching_ignore_terminal_control() {
    let raw = b"\x1b]0;title\x07\x1b[1;32mRouter\x1b[0m\r\nready\x1b]8;;link\x1b\\";
    assert_eq!(terminal_text(raw), "Router\nready");
    assert!(contains_word("Request Rejected.", &["rejected"]));
    assert!(contains_word(
        "Selection not supported here",
        &["not supported"]
    ));
    assert!(!contains_word("errorless terror", &["error"]));
    assert!(shows_trust_dialog("Trustthisfolder?Codexcanread"));
    assert!(shows_trust_dialog("Your trust decision will be saved."));
    assert!(!shows_trust_dialog("› Ask Codex to do anything"));
    assert_eq!(
        name_fragment("router-native-arguments"),
        "router-native-argum"
    );
    assert_eq!(name_fragment("abc"), "");
}

fn cat_terminal(directory: &Path) -> InstalledClientTerminal {
    InstalledClientTerminal::spawn(
        Path::new("/bin/cat"),
        &[],
        directory,
        &[],
        &directory.join("cat.pty"),
    )
    .unwrap()
}

#[test]
fn typed_input_is_captured_and_waited_for() {
    let directory = TempDir::new().unwrap();
    let capture = directory.path().join("cat.pty");
    let mut terminal = cat_terminal(directory.path());
    let mark = terminal.mark();
    terminal.type_text("router-native-probe\r").unwrap();
    let screen = terminal
        .wait_for("router-native-probe", mark, Duration::from_secs(10))
        .unwrap();
    assert!(screen.contains("router-native-probe"));
    assert_eq!(
        terminal.wait_for_effect_or_error("never-printed", mark, Duration::from_millis(300)),
        None
    );
    terminal.shutdown(Duration::from_secs(10)).unwrap();
    assert!(!terminal.forced_stop);
    assert_eq!(terminal.capture_fault, None);
    assert!(String::from_utf8_lossy(&fs::read(&capture).unwrap()).contains("router-native-probe"));
    assert_eq!(
        fs::metadata(&capture).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn missing_screen_text_times_out_instead_of_passing() {
    let directory = TempDir::new().unwrap();
    let mut terminal = cat_terminal(directory.path());
    let mark = terminal.mark();
    assert!(
        terminal
            .wait_for("never-printed", mark, Duration::from_millis(500))
            .is_err()
    );
}

#[test]
fn trust_dialog_stops_all_further_input() {
    let directory = TempDir::new().unwrap();
    let mut terminal = cat_terminal(directory.path());
    let mark = terminal.mark();
    terminal.type_text("Trust this folder?\r").unwrap();
    terminal
        .wait_for("folder?", mark, Duration::from_secs(10))
        .unwrap();
    assert!(terminal.trust_dialog_shown());
    assert!(terminal.send(b"1\r").unwrap_err().contains("trust"));
    // The quit key still works: shutdown never answers a dialog choice.
    terminal.shutdown(Duration::from_secs(10)).unwrap();
}

#[test]
fn existing_capture_is_never_reused() {
    let directory = TempDir::new().unwrap();
    fs::write(directory.path().join("cat.pty"), "earlier").unwrap();
    assert!(
        InstalledClientTerminal::spawn(
            Path::new("/bin/cat"),
            &[],
            directory.path(),
            &[],
            &directory.path().join("cat.pty")
        )
        .is_err()
    );
}
