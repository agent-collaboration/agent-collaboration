use super::*;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn write_limit(directory: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join(FILE_NAME))?
        .write_all(bytes)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
fn private_inbox_limit_is_optional_and_positive() -> TestResult {
    // Arrange an isolated owner-controlled fixture directory without an override.
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    assert_eq!(load(root.path())?, None);

    // Act through the file input the debug Host reads once at construction.
    write_limit(root.path(), br#"{"inboxBytes":1}"#)?;
    let limit = load(root.path())?;
    std::fs::write(root.path().join(FILE_NAME), br#"{"inboxBytes":2}"#)?;

    // Assert the first value remains independent of subsequent file edits.
    assert_eq!(limit.map(NonZeroU64::get), Some(1));
    assert_eq!(load(root.path())?.map(NonZeroU64::get), Some(2));
    for invalid in [
        br#"{"inboxBytes":0}"#.as_slice(),
        br#"{"inboxBytes":-1}"#,
        br#"{"inboxBytes":1,"other":true}"#,
        b"malformed",
    ] {
        std::fs::write(root.path().join(FILE_NAME), invalid)?;
        assert!(
            load(root.path()).is_err(),
            "invalid Inbox limit was accepted"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
fn inbox_limit_rejects_public_or_aliased_fixture_input() -> TestResult {
    // Arrange valid contents so only the filesystem admission can refuse them.
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    write_limit(root.path(), br#"{"inboxBytes":1}"#)?;
    let path = root.path().join(FILE_NAME);
    let original = root.path().join("original.json");

    // Act / Assert each path boundary, returning to valid input between cuts.
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))?;
    assert!(
        load(root.path()).is_err(),
        "public fixture directory was accepted"
    );
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
    assert!(
        load(root.path()).is_err(),
        "public fixture file was accepted"
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&path, &original)?;
    std::os::unix::fs::symlink(&original, &path)?;
    assert!(
        load(root.path()).is_err(),
        "aliased fixture file was accepted"
    );
    assert!(load(Path::new("relative-fixture")).is_err());
    Ok(())
}
