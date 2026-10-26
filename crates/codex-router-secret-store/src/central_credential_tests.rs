#![allow(clippy::panic_in_result_fn)]
use crate::central_credentials::{CentralCredential, CredentialFileError};
use std::{error::Error, os::unix::fs::PermissionsExt};
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
