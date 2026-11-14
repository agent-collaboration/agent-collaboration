//! Parent death must settle the real bridge and its workerd without escaping nextest's group.
use super::*;
use rustix::process::{Pid, test_kill_process};
use std::io::{Read, Write};

const CHILD_MODE: &str = "WORKERD_LIFECYCLE_CHILD";
const READY_PREFIX: &str = "WORKERD_LIFECYCLE_READY ";

#[test]
#[ignore = "explicit lifecycle leak negative control"]
fn lifecycle_leak_negative_control() {
    if std::env::var(CHILD_MODE).as_deref() != Ok("leak") {
        panic!("explicit leak mode required");
    }
    let child = Command::new("node")
        .args([
            "-e",
            "require('node:net').createServer(()=>{}).listen(0,'127.0.0.1');",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    println!("WORKERD_LIFECYCLE_LEAK_PID {}", child.id());
    // Only the explicit negative control deliberately retains nextest's output pipes.
    // Its driver records this PID and kills it after observing the real LEAK result.
    std::mem::forget(child);
}

fn process_id(raw: u32) -> Pid {
    Pid::from_raw(i32::try_from(raw).unwrap()).unwrap()
}

pub(crate) fn workerd_child(bridge: u32) -> u32 {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cannot inspect owned bridge children"
    );
    let children: Vec<u32> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let parent = fields.next()?.parse::<u32>().ok()?;
            (parent == bridge).then_some(pid)
        })
        .collect();
    assert_eq!(children.len(), 1, "expected exactly one workerd child");
    let workerd = children[0];
    let output = Command::new("/bin/ps")
        .args(["-o", "comm=", "-p", &workerd.to_string()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("workerd")
    );
    assert_eq!(
        rustix::process::getpgid(Some(process_id(bridge))).unwrap(),
        rustix::process::getpgid(Some(process_id(workerd))).unwrap(),
        "workerd must retain the test's group"
    );
    workerd
}

#[tokio::test]
#[ignore = "relaunched lifecycle child; requires explicit mode"]
async fn lifecycle_child_owns_real_workerd() {
    let mode = std::env::var(CHILD_MODE).expect("explicit child mode required");
    let clock = Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1_900_000_000_000,
        ))
        .unwrap(),
    );
    let host = WorkerdHost::open(clock).await.unwrap();
    let bridge = host.transport.process.with_child(|child| child.id());
    let workerd = workerd_child(bridge);
    assert_eq!(
        rustix::process::getpgid(Some(process_id(bridge))).unwrap(),
        rustix::process::getpgid(None).unwrap(),
        "bridge must retain its owning test group"
    );
    println!("{READY_PREFIX}{bridge} {workerd}");
    std::io::stdout().flush().unwrap();
    if mode == "timeout" {
        // Only the explicitly selected nextest negative control enters here.
        std::future::pending::<()>().await;
    }
    let mut command = [0];
    std::io::stdin().read_exact(&mut command).unwrap();
    assert_eq!(command, [b'P']);
    panic!("intentional parent panic after real workerd readiness");
}

pub(crate) fn require_exited(bridge: u32, workerd: u32) {
    for pid in [bridge, workerd] {
        assert!(
            matches!(
                test_kill_process(process_id(pid)),
                Err(rustix::io::Errno::SRCH)
            ),
            "owned PID {pid} survived completed bridge settlement"
        );
    }
}

#[tokio::test]
async fn legacy_owner_panic_disposes_real_workerd() {
    use collaboration_central_conformance::{
        JwksFixtureServer,
        workerd_process::{WorkerdProcess, WorkerdSettings},
    };
    let keys = JwksFixtureServer::start().await.unwrap();
    let persistence = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let settings = WorkerdSettings {
        node: "node".into(),
        miniflare: root.join("verification/node_modules/miniflare/dist/src/index.js"),
        persistence: persistence.path().to_owned(),
        port,
        jwks_url: keys.url.clone(),
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
        relay_peer: "019f0000-0000-7000-8000-000000005240"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    let process = WorkerdProcess::start(&settings).await.unwrap();
    let table = Command::new("/bin/ps")
        .args(["-axo", "pid=,args="])
        .output()
        .unwrap();
    assert!(table.status.success());
    let pids: Vec<u32> = String::from_utf8(table.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(' ')?;
            (command.contains("scripts/tooling/run-central-workerd.mjs")
                && command.contains(persistence.path().to_str().unwrap()))
            .then(|| pid.parse().unwrap())
        })
        .collect();
    assert_eq!(
        pids.len(),
        1,
        "one exact legacy bridge owns the unique persistence"
    );
    let bridge = pids[0];
    let workerd = workerd_child(bridge);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _owned = process;
        panic!("intentional legacy owner panic");
    }));
    assert!(panic.is_err());
    require_exited(bridge, workerd);
    keys.shutdown().await.unwrap();
}

#[tokio::test]
async fn real_bridge_eof_disposes_workerd() {
    let clock = Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1_900_000_000_000,
        ))
        .unwrap(),
    );
    let host = WorkerdHost::open(clock).await.unwrap();
    let bridge = host.transport.process.with_child(|child| child.id());
    let workerd = workerd_child(bridge);
    let detected = Command::new("pgrep")
        .args(["-x", "workerd"])
        .output()
        .unwrap();
    assert!(
        detected.status.success(),
        "the CI orphan check sees a real workerd"
    );
    assert!(
        String::from_utf8(detected.stdout)
            .unwrap()
            .lines()
            .any(|pid| pid.trim() == workerd.to_string())
    );
    host.close().await;
    require_exited(bridge, workerd);
}

#[tokio::test]
async fn dropping_host_settles_children_even_when_a_remote_handle_remains() {
    let clock = Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1_900_000_000_000,
        ))
        .unwrap(),
    );
    let host = WorkerdHost::open(clock).await.unwrap();
    let bridge = host.transport.process.with_child(|child| child.id());
    let workerd = workerd_child(bridge);
    let organization = "019f0000-0000-7000-8000-00000000d201"
        .to_owned()
        .try_into()
        .unwrap();
    let retained = host.cold_runtime(&organization);
    drop(host);
    require_exited(bridge, workerd);
    assert!(matches!(
        retained
            .read(Statement {
                sql: "SELECT 1",
                args: vec![]
            })
            .await,
        Err(StoreFailure::Unavailable)
    ));
}

#[test]
fn parent_sigkill_and_panic_leave_no_bridge_or_workerd() {
    let selector = format!(
        "{}::lifecycle_child_owns_real_workerd",
        module_path!().split_once("::").unwrap().1
    );
    run_parent_death(&selector, &["sigkill", "panic"]);
}

pub(crate) fn run_parent_death(selector: &str, modes: &[&str]) {
    for &mode in modes {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", selector, "--ignored", "--nocapture"])
            .env(CHILD_MODE, mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let drain = std::thread::spawn(move || {
            let mut result = String::new();
            BufReader::new(stderr).read_to_string(&mut result).unwrap();
            result
        });
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        let (bridge, workerd) = loop {
            line.clear();
            assert_ne!(
                output.read_line(&mut line).unwrap(),
                0,
                "child selector never reached real readiness"
            );
            if let Some(receipt) = line.trim().strip_prefix(READY_PREFIX) {
                let pids: Vec<u32> = receipt
                    .split_whitespace()
                    .map(|pid| pid.parse().unwrap())
                    .collect();
                assert_eq!(pids.len(), 2);
                break (pids[0], pids[1]);
            }
        };
        if mode == "sigkill" {
            child.kill().unwrap();
        } else {
            child.stdin.take().unwrap().write_all(b"P").unwrap();
        }
        let status = child.wait().unwrap();
        assert!(
            !status.success(),
            "negative-control child unexpectedly passed"
        );
        // EOF on stderr observes both parent and its inheriting bridge exit.
        let _ = drain.join().unwrap();
        require_exited(bridge, workerd);
        let mut remainder = String::new();
        output.read_to_string(&mut remainder).unwrap();
        if mode == "panic" {
            assert!(
                remainder.contains("1 failed"),
                "child selector did not execute its panic case: {remainder}"
            );
        }
        eprintln!("settled parent {mode}: bridge={bridge}, workerd={workerd}");
    }
}
