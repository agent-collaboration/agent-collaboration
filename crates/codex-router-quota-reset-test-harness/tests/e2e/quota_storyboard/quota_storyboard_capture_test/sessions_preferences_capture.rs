//! Validate the explicitly nominated picker-only store directory before any PTY child.
use super::{OsStr, Path, PathBuf, TestResult, ensure};
use std::os::unix::fs::{MetadataExt, PermissionsExt};

pub(super) fn directory(surface: &str, nomination: Option<&OsStr>) -> TestResult<Option<PathBuf>> {
    let Some(nomination) = nomination else {
        return Ok(None);
    };
    ensure(
        surface == "sessions-filters",
        "picker preferences require the named sessions fixture",
    )?;
    let path = PathBuf::from(nomination);
    let lane_tmp = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp"))?;
    let metadata = std::fs::symlink_metadata(&path)?;
    ensure(
        path.is_absolute()
            && std::fs::canonicalize(&path)? == path
            && path.starts_with(&lane_tmp)
            && path != lane_tmp
            && metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == rustix::process::geteuid().as_raw()
            && metadata.permissions().mode() & 0o077 == 0,
        "picker preferences require an owned private canonical directory under lane tmp",
    )?;
    Ok(Some(path))
}

#[test]
fn picker_preferences_validate_private_owned_paths_and_allow_the_same_run_to_reopen()
-> TestResult<()> {
    use std::os::unix::fs::{DirBuilderExt, symlink};
    let lane_tmp = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp"))?;
    let path = lane_tmp.join(format!("grid-picker-preferences-{}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&path)?;
    let link = path.with_extension("link");
    let database = path.join("agent-sessions-ui.sqlite");
    let result = (|| {
        ensure(
            directory("sessions-filters", None)?.is_none(),
            "implicit preferences appeared",
        )?;
        ensure(
            directory("sessions-filters", Some(path.as_os_str()))? == Some(path.clone()),
            "owned preferences rejected",
        )?;
        std::fs::write(&database, "retained first invocation")?;
        ensure(
            directory("sessions-filters", Some(path.as_os_str()))? == Some(path.clone()),
            "same-run reopened store rejected",
        )?;
        for surface in ["sessions", "quota", "normal", "showcase"] {
            ensure(
                directory(surface, Some(path.as_os_str())).is_err(),
                "preferences escaped named picker fixture",
            )?;
        }
        for invalid in [
            PathBuf::from("relative"),
            path.join("missing"),
            database.clone(),
            std::env::temp_dir(),
        ] {
            ensure(
                directory("sessions-filters", Some(invalid.as_os_str())).is_err(),
                "invalid preference path admitted",
            )?;
        }
        symlink(&path, &link)?;
        ensure(
            directory("sessions-filters", Some(link.as_os_str())).is_err(),
            "symlink preferences admitted",
        )?;
        for mode in [0o755, 0o770, 0o707] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
            ensure(
                directory("sessions-filters", Some(path.as_os_str())).is_err(),
                "nonprivate preferences admitted",
            )?;
        }
        Ok(())
    })();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    if link.exists() {
        std::fs::remove_file(link)?;
    }
    if database.exists() {
        std::fs::remove_file(database)?;
    }
    std::fs::remove_dir(path)?;
    result
}
