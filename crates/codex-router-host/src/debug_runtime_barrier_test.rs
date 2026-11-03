use super::*;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
fn debug_reset_barrier_refuses_relative_public_and_aliased_socket_paths() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let socket = root.path().join("barrier.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket)?;
    validate_barrier_socket(&socket)?;
    assert!(validate_barrier_socket(Path::new("barrier.sock")).is_err());
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))?;
    assert!(validate_barrier_socket(&socket).is_err());
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let alias = root.path().join("alias.sock");
    std::os::unix::fs::symlink(&socket, &alias)?;
    assert!(validate_barrier_socket(&alias).is_err());
    drop(listener);
    Ok(())
}
