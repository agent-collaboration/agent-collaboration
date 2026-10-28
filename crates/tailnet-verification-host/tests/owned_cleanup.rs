use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use codex_router_tailnet_verification_host::driver_error::{DriverError, DriverResult};
use codex_router_tailnet_verification_host::owned_host::{
    CleanupInventory, capture_cleanup_inventory, with_owned_child,
};
use codex_router_tailnet_verification_host::process_identity::{
    ProcessIdentity, process_table, require_protected,
};
use codex_router_tailnet_verification_host::profile_projection::ProfileProjection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn harmless_child() -> Result<Child, Box<dyn std::error::Error>> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../verification/fixtures/harmless-owned-child.ts");
    let mut child = Command::new("node")
        .arg(fixture)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("fixture output absent")?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    // Node sets its Linux comm during startup; record ownership only after READY.
    let ready = receiver.recv_timeout(Duration::from_secs(5));
    if !matches!(&ready, Ok(Ok(line)) if line == "READY\n") {
        let _ = child.kill();
        let _ = child.wait();
    }
    reader.join().map_err(|_| "fixture reader failed")?;
    assert_eq!(ready??, "READY\n");
    Ok(child)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn exercise_cleanup(action: &str) -> Result<(), Box<dyn std::error::Error>> {
    let evidence = tempfile::tempdir()?;
    if action == "write-failure" {
        std::fs::create_dir(evidence.path().join("host-before-fallback.json"))?;
    }
    let mut child = harmless_child()?;
    let result = with_owned_child(&mut child, evidence.path(), |child, _known| {
        if action == "body-error" {
            return Err(DriverError::Runtime("body failed"));
        }
        if action == "exited" {
            drop(child.stdin.take());
            child.wait()?;
        }
        Ok(())
    });
    assert!(child.try_wait()?.is_some());
    match action {
        "body-error" => assert_eq!(
            result.err().ok_or("missing body error")?.to_string(),
            "body failed"
        ),
        "write-failure" => assert_eq!(
            result.err().ok_or("missing evidence refusal")?.to_string(),
            "cleanup evidence unavailable; owned cleanup was still attempted"
        ),
        _ => result?,
    }
    let settlement: Value = serde_json::from_slice(&std::fs::read(
        evidence.path().join("host-settlement.json"),
    )?)?;
    assert_eq!(settlement.get("remaining"), Some(&json!({})));
    assert_eq!(settlement.get("errors"), Some(&json!([])));
    if action != "write-failure" {
        let snapshot: Value = serde_json::from_slice(&std::fs::read(
            evidence.path().join("host-before-fallback.json"),
        )?)?;
        assert_eq!(snapshot.get("kind"), Some(&json!("observed")));
        assert!(
            !snapshot
                .get("recorded")
                .and_then(Value::as_array)
                .ok_or("recorded identities absent")?
                .is_empty()
        );
        if action == "exited" {
            assert_eq!(snapshot.get("matching"), Some(&json!([])));
        } else {
            assert_eq!(snapshot.get("recorded"), snapshot.get("matching"));
        }
    }
    Ok(())
}

#[test]
fn body_failure_still_reaps_the_real_owned_child() -> Result<(), Box<dyn std::error::Error>> {
    exercise_cleanup("body-error")
}
#[test]
fn exited_child_has_independent_empty_pre_fallback_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_cleanup("exited")
}
#[test]
fn evidence_write_failure_still_cleans_the_real_owned_child()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_cleanup("write-failure")
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn denied_or_empty_inventory_remains_unavailable_and_protected_mismatch_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let own_pid = i32::try_from(std::process::id())?;
    let identity = process_table()?
        .remove(&own_pid)
        .ok_or("verifier identity absent")?;
    let known = BTreeMap::from([(own_pid, identity.clone())]);
    type InventoryReader = fn() -> DriverResult<BTreeMap<i32, ProcessIdentity>>;
    let observations: [InventoryReader; 2] = [
        || Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()),
        || Ok(BTreeMap::new()),
    ];
    for read in observations {
        let snapshot = capture_cleanup_inventory(&known, read);
        assert!(matches!(snapshot, CleanupInventory::Unavailable { .. }));
        let raw = serde_json::to_value(snapshot)?;
        assert_eq!(raw.get("recorded"), Some(&json!([identity])));
        assert!(raw.get("matching").is_none());
    }
    require_protected(std::slice::from_ref(&identity))?;
    let changed = ProcessIdentity {
        birth: "wrong birth".to_owned(),
        ..identity
    };
    assert!(require_protected(&[changed]).is_err());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn composed_body_error_cleans_child_before_exact_profile_restore()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let profile = root.path().join("profile");
    let original = b"base_url = \"http://127.0.0.1:18787/v1\"\n";
    std::fs::write(&profile, original)?;
    let projection =
        ProfileProjection::acquire(&profile, &format!("{:x}", Sha256::digest(original)), 43128)?;
    let mut child = harmless_child()?;
    let result: DriverResult<()> = with_owned_child(&mut child, root.path(), |_child, _known| {
        Err(DriverError::Runtime("composed error"))
    });
    assert!(child.try_wait()?.is_some());
    projection.restore()?;
    assert_eq!(
        result.err().ok_or("body error absent")?.to_string(),
        "composed error"
    );
    assert_eq!(std::fs::read(profile)?, original);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn native_launcher_creates_the_same_session_and_group_as_its_host_pid()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../verification/fixtures/harmless-owned-child.ts");
    let mut child = Command::new(env!("CARGO_BIN_EXE_codex-router-tailnet-verification-host"))
        .arg("__launch-host")
        .arg("node")
        .arg(fixture)
        .arg("--announce-ready")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("fixture output absent")?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let ready = receiver.recv_timeout(Duration::from_secs(5));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(ready??, "READY\n");
        let pid = rustix::process::Pid::from_raw(i32::try_from(child.id())?)
            .ok_or("child PID invalid")?;
        assert_eq!(rustix::process::getsid(Some(pid))?, pid);
        assert_eq!(rustix::process::getpgid(Some(pid))?, pid);
        Ok(())
    })();
    drop(child.stdin.take());
    if result.is_err() {
        let _ = child.kill();
    }
    child.wait()?;
    reader.join().map_err(|_| "fixture reader failed")?;
    result
}
