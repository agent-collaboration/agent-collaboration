#![allow(clippy::panic_in_result_fn)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

use codex_router_tailnet_verification_host::profile_projection::{
    ProfileGuardError, ProfileProjection,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct Failure {
    category: String,
    message: Option<String>,
}
#[derive(Deserialize)]
struct ProfileCase {
    action: String,
    original: String,
    hash: String,
    projected: Option<String>,
    after: String,
    failure: Option<Failure>,
    contention: Option<String>,
}

#[test]
fn matches_ten_original_disposable_profile_scenarios() -> Result<(), Box<dyn std::error::Error>> {
    let cases: Vec<ProfileCase> = serde_json::from_str(include_str!("profile-cases.json"))?;
    for case in cases {
        let root = tempfile::tempdir()?;
        let path = root.path().join("profile");
        fs::write(&path, &case.original)?;
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(if case.action == "shared-write" {
                0o666
            } else {
                0o600
            }),
        )?;
        let target = if case.action == "symlink" {
            let alias = root.path().join("alias");
            symlink(&path, &alias)?;
            alias
        } else {
            path.clone()
        };
        let mut projected = None;
        let mut contention = None;
        let outcome = ProfileProjection::acquire(&target, &case.hash, 43128).and_then(|guard| {
            projected = Some(fs::read_to_string(&path)?);
            let body = match case.action.as_str() {
                "body-error" => Err(ProfileGuardError::Invalid("body failed")),
                "changed-bytes" => {
                    fs::write(&path, "newer owner bytes").map_err(ProfileGuardError::from)
                }
                "changed-inode" => {
                    fs::remove_file(&path)?;
                    fs::write(&path, "newer owner inode").map_err(ProfileGuardError::from)
                }
                "lock-contention" => {
                    if let Err(error) = ProfileProjection::acquire(&path, &case.hash, 43129) {
                        contention = Some(error.category().to_owned());
                    }
                    Ok(())
                }
                _ => Ok(()),
            };
            guard.restore()?;
            body
        });
        let failure = outcome.err().map(|error| Failure {
            category: if case.action == "body-error" {
                "RuntimeError"
            } else {
                error.category()
            }
            .to_owned(),
            message: matches!(
                &error,
                ProfileGuardError::Invalid(_) | ProfileGuardError::Conflict
            )
            .then(|| error.to_string()),
        });
        assert_eq!(projected, case.projected, "{} projection", case.action);
        assert_eq!(
            fs::read_to_string(&path)?,
            case.after,
            "{} final bytes",
            case.action
        );
        assert_eq!(failure, case.failure, "{} refusal", case.action);
        assert_eq!(
            contention, case.contention,
            "{} lock contention",
            case.action
        );
    }
    Ok(())
}
