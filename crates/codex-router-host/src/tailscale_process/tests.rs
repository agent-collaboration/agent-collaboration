use super::*;
use std::{num::NonZeroU16, path::Path};

// The test double is the foreign CLI only. Spawn, pipes, waits, signals and reaping stay real.
const CHILD_FIXTURE: &str = r#"
import json, os, signal, socket, sys
mode = os.environ['TAILNET_TEST_MODE']
if mode == 'finite_failure':
    print('PRIVATE_FIXTURE_OUTPUT', file=sys.stderr)
    sys.exit(23)
if mode == 'finite_overflow':
    print('PRIVATE_FIXTURE_OUTPUT' * 65537)
    sys.exit(0)
if mode == 'finite_hang':
    while True:
        signal.pause()
if sys.argv[1:] == ['status', '--json']:
    if mode == 'overflow':
        print('x' * 65537)
    elif mode == 'bad_status':
        print('not-json')
    else:
        print(json.dumps({'BackendState':'Running','Self':{'DNSName':'node.example.ts.net.'},'CertDomains':['node.example.ts.net']}))
    sys.exit(0)
if mode == 'early_exit':
    sys.exit(23)
signal.signal(signal.SIGINT, lambda *_: sys.exit(0))
print('https://node.example.ts.net:8443/', flush=True)
if mode == 'controlled_exit':
    connection = socket.socket(socket.AF_UNIX)
    connection.connect(os.environ['TAILNET_TEST_CONTROL'])
    connection.sendall(b'ready')
    connection.recv(1)
    sys.exit(23)
while True:
    signal.pause()
"#;

fn command_fixture(mode: &str, control: Option<&Path>) -> TailscaleCommand {
    let mut specification = ChildCommandSpec::new(PathBuf::from("/usr/bin/python3"))
        .with_arguments(["-u", "-c", CHILD_FIXTURE])
        .with_environment("TAILNET_TEST_MODE", mode);
    if let Some(control) = control {
        specification = specification.with_environment("TAILNET_TEST_CONTROL", control);
    }
    TailscaleCommand::fixture(specification)
}

fn port() -> NonZeroU16 {
    NonZeroU16::new(8443).expect("port")
}

fn assert_reaped(process_id: u32) {
    let process_id = i32::try_from(process_id).expect("pid fits");
    let pid = rustix::process::Pid::from_raw(process_id).expect("positive pid");
    assert!(
        rustix::process::test_kill_process(pid).is_err(),
        "owned child survived its explicit shutdown"
    );
}

#[test]
fn serve_argv_is_literal_foreground_https_root_and_actual_loopback_backend() {
    assert_eq!(
        serve_arguments(port(), "127.0.0.1:19088".parse().expect("backend")),
        [
            "serve",
            "--bg=false",
            "--https=8443",
            "--set-path=/",
            "http://127.0.0.1:19088",
        ]
        .map(OsString::from)
    );
}

#[tokio::test]
async fn finite_status_calls_bound_output_and_reject_malformed_status_at_the_parser() {
    let status = command_fixture("normal", None)
        .capture(&["status", "--json"])
        .await
        .expect("real status subprocess");
    assert_eq!(
        crate::tailscale_status::https_node_name(&status).expect("status"),
        "node.example.ts.net"
    );
    let bad = command_fixture("bad_status", None)
        .capture(&["status", "--json"])
        .await
        .expect("subprocess exits successfully");
    assert!(crate::tailscale_status::https_node_name(&bad).is_err());
    assert!(
        command_fixture("overflow", None)
            .capture(&["status", "--json"])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn startup_failure_is_reaped_without_a_ready_receipt() {
    let mut process = command_fixture("early_exit", None)
        .spawn_serve(
            port(),
            "127.0.0.1:19088".parse().expect("backend"),
            "node.example.ts.net",
        )
        .expect("spawn");
    let process_id = process.process_id().expect("pid");
    assert!(process.advertised().await.is_err());
    process.stop().await.expect("settle failed startup");
    assert_reaped(process_id);
}

#[tokio::test]
async fn normal_shutdown_reaps_the_advertised_foreground_child() {
    let mut process = command_fixture("normal", None)
        .spawn_serve(
            port(),
            "127.0.0.1:19088".parse().expect("backend"),
            "node.example.ts.net",
        )
        .expect("spawn");
    let process_id = process.process_id().expect("pid");
    process.advertised().await.expect("advertised");
    process.stop().await.expect("graceful child shutdown");
    assert_reaped(process_id);
}

#[tokio::test]
async fn exit_after_advertisement_is_a_failure_and_reaps_the_same_child() {
    let directory = tempfile::tempdir_in("/tmp").expect("short socket directory");
    let control = directory.path().join("exit.sock");
    let listener = tokio::net::UnixListener::bind(&control).expect("control listener");
    let mut process = command_fixture("controlled_exit", Some(&control))
        .spawn_serve(
            port(),
            "127.0.0.1:19088".parse().expect("backend"),
            "node.example.ts.net",
        )
        .expect("spawn");
    let process_id = process.process_id().expect("pid");
    process
        .advertised()
        .await
        .expect("advertised before exit event");
    let (mut connection, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .expect("control bound")
        .expect("control");
    tokio::io::AsyncWriteExt::write_all(&mut connection, b"x")
        .await
        .expect("exit event");
    let failure = tokio::time::timeout(Duration::from_secs(5), process.failure())
        .await
        .expect("exit observed");
    assert!(
        failure.to_string().contains("exited unexpectedly"),
        "{failure}"
    );
    process.stop().await.expect("settle observed exit");
    assert_reaped(process_id);
}

#[tokio::test]
async fn cancelling_an_observation_keeps_the_child_owned_until_explicit_shutdown() {
    let mut process = command_fixture("normal", None)
        .spawn_serve(
            port(),
            "127.0.0.1:19088".parse().expect("backend"),
            "node.example.ts.net",
        )
        .expect("spawn");
    let process_id = process.process_id().expect("pid");
    process.advertised().await.expect("advertised");
    // Cancel a genuinely polled observation without sleeping or releasing ownership.
    {
        let observation = process.failure();
        tokio::pin!(observation);
        assert!(futures_util::poll!(&mut observation).is_pending());
    }
    process
        .stop()
        .await
        .expect("explicit owner shutdown after cancellation");
    assert_reaped(process_id);
}

async fn assert_finite_failure_context(mode: &str, phase: &str) {
    for (arguments, command_name) in [
        (vec!["status", "--json"], "tailscale status --json"),
        (
            vec!["serve", "status", "--json"],
            "tailscale serve status --json",
        ),
    ] {
        let failure = command_fixture(mode, None)
            .capture(&arguments)
            .await
            .expect_err("negative foreign CLI fixture");
        let message = failure.to_string();
        assert!(
            message.contains(command_name),
            "missing command context: {message}"
        );
        assert!(message.contains(phase), "missing phase context: {message}");
        assert!(
            !message.contains("PRIVATE_FIXTURE_OUTPUT"),
            "output leaked: {message}"
        );
        if mode == "finite_hang" {
            assert_eq!(failure.kind(), io::ErrorKind::TimedOut);
            assert!(message.contains("stdout pending"), "{message}");
            assert!(message.contains("stderr pending"), "{message}");
            assert!(message.contains("exit pending"), "{message}");
            assert!(message.contains("no command was retried"), "{message}");
        }
    }
}

#[tokio::test]
async fn finite_failure_identifies_command_and_exit_phase_without_output() {
    assert_finite_failure_context("finite_failure", "exit").await;
}

#[tokio::test]
async fn finite_overflow_identifies_command_and_stdout_phase_without_output() {
    assert_finite_failure_context("finite_overflow", "stdout").await;
}

#[tokio::test]
async fn finite_timeout_identifies_command_and_pending_phases_without_retry() {
    assert_finite_failure_context("finite_hang", "timed out").await;
}

#[cfg(all(test, target_os = "linux"))]
#[derive(Clone, Copy, Debug)]
pub(super) enum ServeSigintSendOutcome {
    PidUnavailable,
    Sent,
    Failed,
}

#[derive(Debug, Eq, PartialEq)]
enum ProcSignalBit {
    Set,
    Clear,
    Unknown,
}

#[derive(Debug, Eq, PartialEq)]
enum ProcStateClass {
    Running,
    Sleeping,
    DiskSleep,
    Stopped,
    Zombie,
    Other,
    Unknown,
}

#[derive(Debug, Eq, PartialEq)]
enum ProcWaitSite {
    Pause,
    Other,
    Unavailable,
}

#[cfg(all(test, target_os = "linux"))]
pub(super) fn report_linux_serve_shutdown_timeout(
    owned_pid: Option<u32>,
    signal_send: ServeSigintSendOutcome,
) {
    use std::io::Write as _;
    // Only inspect this still-owned child after the original wait failed. No
    // pre-signal reads may perturb the fixture's signal/pause interleaving.
    let status = owned_pid.and_then(|pid| bounded_proc_text(&format!("/proc/{pid}/status"), 8192));
    let wait_site = owned_pid.and_then(|pid| bounded_proc_text(&format!("/proc/{pid}/wchan"), 256));
    let status = status.as_deref().unwrap_or("");
    // Every printed value is a closed enum or a boolean. Never print proc
    // contents, process IDs, addresses, argv, names or environment values.
    let _diagnostic_result = writeln!(
        std::io::stderr().lock(),
        "tailscale_owned_serve_shutdown_timeout pid_available={} signal_send={signal_send:?} state={:?} sigint_blocked={:?} sigint_ignored={:?} sigint_caught={:?} thread_pending_sigint={:?} shared_pending_sigint={:?} wait_site={:?}",
        owned_pid.is_some(),
        proc_state_class(status),
        proc_sigint_bit(status, "SigBlk"),
        proc_sigint_bit(status, "SigIgn"),
        proc_sigint_bit(status, "SigCgt"),
        proc_sigint_bit(status, "SigPnd"),
        proc_sigint_bit(status, "ShdPnd"),
        proc_wait_site(wait_site.as_deref()),
    );
}

#[cfg(all(test, target_os = "linux"))]
fn bounded_proc_text(path: &str, limit: u64) -> Option<String> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn proc_status_field<'a>(status: &'a str, field: &str) -> Option<&'a str> {
    let mut values = status.lines().filter_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name == field).then_some(value.trim())
    });
    match (values.next(), values.next()) {
        (Some(value), None) => Some(value),
        _ => None,
    }
}

fn proc_sigint_bit(status: &str, field: &str) -> ProcSignalBit {
    let parsed = proc_status_field(status, field)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 16
                && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .and_then(|value| u64::from_str_radix(value, 16).ok());
    let Some(mask) = parsed else {
        return ProcSignalBit::Unknown;
    };
    let Some(bit) = u32::try_from(rustix::process::Signal::INT.as_raw() - 1)
        .ok()
        .and_then(|shift| 1_u64.checked_shl(shift))
    else {
        return ProcSignalBit::Unknown;
    };
    if mask & bit == 0 {
        ProcSignalBit::Clear
    } else {
        ProcSignalBit::Set
    }
}

fn proc_state_class(status: &str) -> ProcStateClass {
    match proc_status_field(status, "State").and_then(|value| value.split_whitespace().next()) {
        Some("R") => ProcStateClass::Running,
        Some("S") => ProcStateClass::Sleeping,
        Some("D") => ProcStateClass::DiskSleep,
        Some("T" | "t") => ProcStateClass::Stopped,
        Some("Z") => ProcStateClass::Zombie,
        Some("X" | "x" | "K" | "W" | "P" | "I") => ProcStateClass::Other,
        _ => ProcStateClass::Unknown,
    }
}

fn proc_wait_site(wait_site: Option<&str>) -> ProcWaitSite {
    match wait_site.map(str::trim) {
        Some(
            "pause" | "do_pause" | "do_sys_pause" | "__do_sys_pause" | "sys_pause"
            | "__x64_sys_pause" | "__arm64_sys_pause" | "__se_sys_pause",
        ) => ProcWaitSite::Pause,
        Some("" | "0") | None => ProcWaitSite::Unavailable,
        Some(_) => ProcWaitSite::Other,
    }
}

#[test]
fn shutdown_proc_diagnostic_distinguishes_sigint_from_other_signal_bits() {
    let status = "Name:\tPRIVATE_FIXTURE_NAME\nState:\tS (sleeping)\nSigBlk:\t0000000000000002\nSigIgn:\t0000000000000004\nSigCgt:\t0000000000000002\nSigPnd:\t0000000000000000\nShdPnd:\t0000000000000002\n";
    assert_eq!(proc_state_class(status), ProcStateClass::Sleeping);
    assert_eq!(proc_sigint_bit(status, "SigBlk"), ProcSignalBit::Set);
    assert_eq!(proc_sigint_bit(status, "SigIgn"), ProcSignalBit::Clear);
    assert_eq!(proc_sigint_bit(status, "SigCgt"), ProcSignalBit::Set);
    assert_eq!(proc_sigint_bit(status, "SigPnd"), ProcSignalBit::Clear);
    assert_eq!(proc_sigint_bit(status, "ShdPnd"), ProcSignalBit::Set);
    assert_eq!(
        proc_wait_site(Some("__do_sys_pause\n")),
        ProcWaitSite::Pause
    );
    assert_eq!(
        proc_wait_site(Some("PRIVATE_OTHER_WAIT_SITE")),
        ProcWaitSite::Other
    );
}

#[test]
fn shutdown_proc_diagnostic_keeps_missing_or_invalid_signal_data_unknown() {
    for status in [
        "",
        "SigBlk: invalid",
        "SigBlk: +2",
        "SigBlk: 10000000000000000",
        "SigBlk: 0\nSigBlk: 2",
    ] {
        assert_eq!(proc_sigint_bit(status, "SigBlk"), ProcSignalBit::Unknown);
    }
    assert_eq!(proc_state_class(""), ProcStateClass::Unknown);
    assert_eq!(proc_state_class("State: invalid"), ProcStateClass::Unknown);
    for wait_site in [None, Some(""), Some("0\n")] {
        assert_eq!(proc_wait_site(wait_site), ProcWaitSite::Unavailable);
    }
}
