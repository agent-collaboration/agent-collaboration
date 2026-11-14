//! Parent death must settle the real bridge and its workerd without escaping nextest's group.
use super::*;
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const CHILD_MODE: &str = "WORKERD_LIFECYCLE_CHILD";
const READY_PREFIX: &str = "WORKERD_LIFECYCLE_READY ";

#[test]
fn lifecycle_leak_negative_control() {
    if std::env::var(CHILD_MODE).as_deref() != Ok("leak") {
        return;
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

#[test]
fn stop_reaps_bridge_that_ignores_stdin_eof() {
    let mut child = Command::new("node")
        .args(["-e", "process.stdin.resume();require('node:net').createServer(()=>{}).listen(0,'127.0.0.1',()=>console.log('READY'));"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready).unwrap();
    assert_eq!(ready.trim(), "READY");
    let pid = process_id(child.id());
    let process = Process {
        child: Mutex::new(Some(child)),
        output: Mutex::new(None),
        errors: Mutex::new(None),
    };
    assert!(
        !process.stop(),
        "forced stop must retain the unsuccessful child status"
    );
    assert!(
        matches!(test_kill_process(pid), Err(rustix::io::Errno::SRCH)),
        "owned bridge must be reaped"
    );
}

fn process_id(raw: u32) -> Pid {
    Pid::from_raw(i32::try_from(raw).unwrap()).unwrap()
}

fn workerd_child(bridge: u32) -> u32 {
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
async fn lifecycle_child_owns_real_workerd() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    let clock = Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1_900_000_000_000,
        ))
        .unwrap(),
    );
    let host = WorkerdHost::open(clock).await.unwrap();
    let bridge = host
        .transport
        .process
        .child
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .id();
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

fn require_exited(bridge: u32, workerd: u32) {
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = cancelled.clone();
    let observer = std::thread::spawn(move || {
        while !cancellation.load(Ordering::SeqCst) {
            let both_exited = [bridge, workerd].into_iter().all(|pid| {
                matches!(
                    test_kill_process(process_id(pid)),
                    Err(rustix::io::Errno::SRCH)
                )
            });
            if both_exited {
                let _ = sender.send(());
                return;
            }
            std::thread::yield_now();
        }
    });
    let result = receiver.recv_timeout(Duration::from_secs(10));
    if result.is_err() {
        // Cleanup only the two PIDs retained from this owned child, then fail.
        for pid in [bridge, workerd] {
            let _ = kill_process(process_id(pid), Signal::KILL);
        }
    }
    cancelled.store(true, Ordering::SeqCst);
    drop(receiver);
    observer.join().unwrap();
    assert!(
        result.is_ok(),
        "owned bridge/workerd survived parent death: {bridge}/{workerd}"
    );
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
    let bridge = host
        .transport
        .process
        .child
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .id();
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
    for mode in ["sigkill", "panic"] {
        let selector = format!(
            "{}::lifecycle_child_owns_real_workerd",
            module_path!().split_once("::").unwrap().1
        );
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &selector, "--nocapture"])
            .env(CHILD_MODE, mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
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
