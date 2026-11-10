use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn requires_separate_sealed_b_and_writable_a_with_bound_names() -> ObservationResult<()> {
    let directory = tempfile::tempdir()?;
    let directory = std::fs::canonicalize(directory.path())?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let scratch = directory.join("A.sqlite");
    let untouched = directory.join("B.sqlite");
    std::fs::write(&scratch, b"scratch")?;
    std::fs::write(&untouched, b"sealed")?;
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o600))?;
    std::fs::set_permissions(&untouched, std::fs::Permissions::from_mode(0o400))?;
    assert!(qualify_cut(&scratch, &untouched).is_ok());
    assert!(qualify_cut(&untouched, &untouched).is_err());
    let live_name = directory.join("e1.sqlite");
    std::fs::rename(&scratch, &live_name)?;
    assert!(qualify_cut(&live_name, &untouched).is_err());
    Ok(())
}
