#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
use crate::central_credentials::{CentralCredential, CredentialFileError};
use std::{
    error::Error,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
type TestResult = Result<(), Box<dyn Error>>;
fn owner() -> CentralCredential {
    CentralCredential::Owner {
        endpoint: "http://127.0.0.1:19843".into(),
        assertion: "private.fixture.assertion".into(),
        expires_at: "2030-01-01T00:00:00Z".into(),
    }
}
#[test]
fn a_handoff_is_created_private_consumed_once_and_removed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("credential.json");
    owner().write_private_file(&file)?;
    assert_eq!(
        std::fs::metadata(&file)?.permissions().mode() & 0o777,
        0o600
    );
    let credential = CentralCredential::consume_private_file(&file)?;
    assert!(
        matches!(credential, CentralCredential::Owner { assertion, .. } if assertion == "private.fixture.assertion")
    );
    assert!(!file.exists());
    assert!(CentralCredential::consume_private_file(&file).is_err());
    Ok(())
}
#[test]
fn shared_symlinked_hardlinked_oversized_and_malformed_handoffs_fail_closed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("credential.json");
    owner().write_private_file(&file)?;
    assert!(owner().write_private_file(&file).is_err());
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    assert!(matches!(
        CentralCredential::consume_private_file(&file),
        Err(CredentialFileError::Permissions)
    ));
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    let link = directory.path().join("linked.json");
    std::os::unix::fs::symlink(&file, &link)?;
    assert!(matches!(
        CentralCredential::consume_private_file(&link),
        Err(CredentialFileError::Permissions)
    ));
    let hard = directory.path().join("hard.json");
    std::fs::hard_link(&file, &hard)?;
    assert!(matches!(
        CentralCredential::consume_private_file(&hard),
        Err(CredentialFileError::Permissions)
    ));
    std::fs::remove_file(hard)?;
    std::fs::write(&file, vec![b'x'; 65537])?;
    assert!(matches!(
        CentralCredential::consume_private_file(&file),
        Err(CredentialFileError::Permissions)
    ));
    std::fs::write(&file, "{private-secret-canary")?;
    let error = CentralCredential::consume_private_file(&file)
        .err()
        .ok_or("malformed handoff accepted")?;
    assert!(!error.to_string().contains("private-secret-canary"));
    assert!(matches!(error, CredentialFileError::Malformed));
    assert!(
        CentralCredential::consume_private_file(std::path::Path::new("relative.json")).is_err()
    );
    Ok(())
}

#[test]
fn fifo_handoff_is_refused_without_waiting_for_a_writer() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fifo = directory.path().join("credential.fifo");
    create_fifo(&fifo)?;
    require_prompt_refusal(&fifo)?;
    assert!(fifo.exists());
    Ok(())
}

#[test]
fn symlink_to_fifo_is_refused_without_waiting_for_a_writer() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fifo = directory.path().join("credential.fifo");
    create_fifo(&fifo)?;
    let link = directory.path().join("credential.json");
    std::os::unix::fs::symlink(&fifo, &link)?;
    require_prompt_refusal(&link)?;
    assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert!(fifo.exists());
    Ok(())
}

struct HandoffChild(Child);

fn create_fifo(path: &Path) -> TestResult {
    if !Command::new("mkfifo").arg(path).status()?.success() {
        return Err("owned FIFO fixture creation failed".into());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

impl Drop for HandoffChild {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _reaped = self.0.wait();
    }
}

fn require_prompt_refusal(path: &Path) -> TestResult {
    let mut child = HandoffChild(
        Command::new(std::env::current_exe()?)
            .args([
                "--ignored",
                "--exact",
                "central_credential_tests::nonregular_handoff_child",
            ])
            .env("CODEX_ROUTER_CREDENTIAL_HANDOFF_TEST_PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait()? {
            assert!(status.success(), "nonregular handoff was accepted");
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("credential reader blocked on a nonregular handoff".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "subprocess helper invoked by bounded FIFO handoff tests"]
fn nonregular_handoff_child() -> TestResult {
    let path = std::env::var_os("CODEX_ROUTER_CREDENTIAL_HANDOFF_TEST_PATH")
        .ok_or("owned handoff fixture path missing")?;
    if CentralCredential::consume_private_file(Path::new(&path)).is_ok() {
        return Err("nonregular handoff must be refused".into());
    }
    Ok(())
}
