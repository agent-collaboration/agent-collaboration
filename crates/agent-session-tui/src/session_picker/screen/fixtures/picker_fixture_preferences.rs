//! Debug fixtures may persist only in an explicitly supplied private directory.
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub(crate) fn directory() -> io::Result<Option<PathBuf>> {
    checked_directory(std::env::var_os("TUI_SESSIONS_FIXTURE_PREFERENCES_DIR"))
}

fn checked_directory(value: Option<OsString>) -> io::Result<Option<PathBuf>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let directory = PathBuf::from(value);
    if !directory.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fixture preferences directory must be absolute",
        ));
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fixture preferences directory must be an existing owner-only non-symlinked directory",
        ));
    }
    Ok(Some(directory))
}

#[cfg(test)]
mod tests {
    use crate::session_picker::screen::fixtures::picker_fixture_preferences::*;

    #[test]
    fn absent_fixture_preferences_are_transient_without_a_default_path() {
        assert_eq!(checked_directory(None).unwrap(), None);
    }

    #[test]
    fn fixture_preferences_accept_only_an_existing_private_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            checked_directory(Some(root.path().as_os_str().to_owned())).unwrap(),
            Some(root.path().to_owned())
        );
        for mode in [0o755, 0o770, 0o707] {
            fs::set_permissions(root.path(), fs::Permissions::from_mode(mode)).unwrap();
            assert!(checked_directory(Some(root.path().as_os_str().to_owned())).is_err());
        }
    }

    #[test]
    fn fixture_preferences_reject_relative_missing_file_and_symlink_paths() {
        assert!(checked_directory(Some("relative".into())).is_err());
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        assert!(checked_directory(Some(missing.into_os_string())).is_err());
        let file = root.path().join("file");
        fs::write(&file, b"fixture").unwrap();
        assert!(checked_directory(Some(file.into_os_string())).is_err());
        let link = root.path().join("link");
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        assert!(checked_directory(Some(link.into_os_string())).is_err());
    }
}
