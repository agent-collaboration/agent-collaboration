use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::with_projected_profile;
use crate::bounded_command::{install_termination_handler, require_continuing, wait_for_child};
use crate::driver_error::DriverError;
use crate::driver_records::save_record;
use crate::owned_host::with_owned_child;
use crate::process_identity::{ProcessIdentity, lookup_identity};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const FIXTURE_ROOT: &str = "TAILNET_SIGNAL_PROFILE_TEST_ROOT";
const FIXTURE_TEST: &str = "host_lifecycle::interrupt_tests::interrupted_profile_owner_fixture";
const ORIGINAL: &[u8] = b"base_url = \"http://127.0.0.1:18787/v1\"\r\n# original bytes\r\n";

fn await_owned_identity(
    root: &Path,
    driver: &mut Child,
) -> Result<ProcessIdentity, Box<dyn std::error::Error>> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(bytes) = fs::read(root.join("ready.json"))
            && let Ok(identity) = serde_json::from_slice(&bytes)
        {
            return Ok(identity);
        }
        if driver.try_wait()?.is_some() || Instant::now() >= until {
            return Err("signal fixture exited or missed its readiness deadline".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn cleanup_fixture(root: &Path, driver: &mut Child) -> TestResult {
    if driver.try_wait()?.is_none() {
        driver.kill()?;
        driver.wait()?;
    }
    // A negative-control run can orphan only the exact identity this fixture saved.
    if let Ok(bytes) = fs::read(root.join("host-started.json"))
        && let Ok(recorded) = serde_json::from_slice::<ProcessIdentity>(&bytes)
        && lookup_identity(recorded.pid)?.is_some_and(|current| current.same_process(&recorded))
    {
        let pid =
            rustix::process::Pid::from_raw(recorded.pid).ok_or("invalid recorded fixture PID")?;
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
    }
    Ok(())
}

#[test]
fn sigint_reaps_owned_child_before_restoring_the_original_profile() -> TestResult {
    let root = tempfile::Builder::new()
        .prefix("tailnet-interrupt-")
        .tempdir()?;
    fs::write(root.path().join("profile"), ORIGINAL)?;
    fs::set_permissions(
        root.path().join("profile"),
        fs::Permissions::from_mode(0o600),
    )?;
    let mut driver = Command::new(std::env::current_exe()?)
        .args([
            "--ignored",
            "--exact",
            FIXTURE_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(FIXTURE_ROOT, root.path())
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| -> TestResult {
        let identity = await_owned_identity(root.path(), &mut driver)?;
        if fs::read(root.path().join("profile"))? == ORIGINAL {
            return Err("fixture never projected the disposable profile".into());
        }
        let driver_pid = rustix::process::Pid::from_raw(i32::try_from(driver.id())?)
            .ok_or("invalid driver PID")?;
        rustix::process::kill_process(driver_pid, rustix::process::Signal::INT)?;
        let status = wait_for_child(&mut driver, Duration::from_secs(10))?
            .ok_or("signal owner did not finish settlement")?;
        let reaped = lookup_identity(identity.pid)?.is_none();
        let restored = fs::read(root.path().join("profile"))? == ORIGINAL;
        if !status.success() || !reaped || !restored {
            return Err(format!(
                "SIGINT cleanup: driverSettled={}, childReaped={reaped}, profileRestored={restored}",
                status.success(),
            ).into());
        }
        let settlement: Value =
            serde_json::from_slice(&fs::read(root.path().join("host-settlement.json"))?)?;
        if settlement.get("remaining") != Some(&json!({}))
            || settlement.get("errors") != Some(&json!([]))
            || !root.path().join("finished.json").is_file()
        {
            return Err("signal owner exited before successful settlement evidence".into());
        }
        Ok(())
    })();
    let cleanup = cleanup_fixture(root.path(), &mut driver);
    result?;
    cleanup
}

#[test]
#[ignore = "controlled signal-owner subprocess; exercised by the parent signal test"]
fn interrupted_profile_owner_fixture() -> TestResult {
    let root =
        PathBuf::from(std::env::var_os(FIXTURE_ROOT).ok_or("private fixture root required")?);
    let metadata = fs::symlink_metadata(&root)?;
    if !root.is_absolute() || !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err("fixture root must be a private directory".into());
    }
    install_termination_handler()?;
    let result = with_projected_profile(
        &root.join("profile"),
        &format!("{:x}", Sha256::digest(ORIGINAL)),
        43128,
        || {
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../verification/fixtures/harmless-owned-child.ts");
            let mut child = Command::new("node")
                .arg(fixture)
                .args(["--graceful", "--hold-until-signalled"])
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()?;
            with_owned_child(&mut child, &root, |child, known| {
                let pid = i32::try_from(child.id())
                    .map_err(|_| DriverError::Runtime("fixture PID overflow"))?;
                let identity = known
                    .get(&pid)
                    .ok_or(DriverError::Runtime("fixture identity absent"))?;
                save_record(&root.join("ready.json"), identity)?;
                loop {
                    require_continuing()?;
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
        },
    );
    if !matches!(result, Err(DriverError::Interrupted)) {
        return Err(format!("expected owned signal interruption, got {result:?}").into());
    }
    save_record(&root.join("finished.json"), &json!({"settled":true}))?;
    Ok(())
}
