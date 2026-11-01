//! Opt-in scenarios and private artifact paths belong only to the ignored fixture entry.
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Scenario {
    CapturedActions,
    GroupingReopen,
    RemoveCancel,
    RemoveMiddle,
    RemoveLast,
    RemoveOnly,
    RemoveHeld,
    RemoveRequiresDisabled,
    LoginSucceeded,
    LoginCancelled130,
    LoginFailed7,
    RefreshAllSuccess,
    RefreshPartial,
    RefreshAllFailed,
    RefreshSetupFailed,
}
impl Scenario {
    pub(super) fn is_login(self) -> bool {
        matches!(
            self,
            Self::LoginSucceeded | Self::LoginCancelled130 | Self::LoginFailed7
        )
    }
    pub(super) fn requires_login_standin(self) -> bool {
        self.is_login() || self == Self::GroupingReopen
    }
    pub(super) fn is_refresh(self) -> bool {
        matches!(
            self,
            Self::RefreshAllSuccess
                | Self::RefreshPartial
                | Self::RefreshAllFailed
                | Self::RefreshSetupFailed
        )
    }
    pub(super) fn is_removal(self) -> bool {
        matches!(
            self,
            Self::RemoveCancel
                | Self::RemoveMiddle
                | Self::RemoveLast
                | Self::RemoveOnly
                | Self::RemoveHeld
                | Self::RemoveRequiresDisabled
        )
    }
    pub(super) fn account_count(self) -> usize {
        match self {
            Self::RemoveOnly => 1,
            Self::RemoveMiddle | Self::RemoveLast | Self::RemoveHeld => 3,
            _ => 2,
        }
    }
    pub(super) fn captured_position(self) -> usize {
        match self {
            Self::RemoveMiddle | Self::RemoveHeld => 1,
            Self::RemoveLast => 2,
            _ => 0,
        }
    }
    pub(super) fn login_exit(self) -> Option<u8> {
        match self {
            Self::LoginSucceeded => Some(0),
            Self::LoginCancelled130 => Some(130),
            Self::LoginFailed7 => Some(7),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FixtureConfiguration {
    pub scenario: Scenario,
    pub output_directory: PathBuf,
    pub events: PathBuf,
    pub receipt: PathBuf,
    pub control_socket: PathBuf,
    pub dispatch_receipt: PathBuf,
    pub login_receipt: Option<PathBuf>,
    pub login_gate_socket: Option<PathBuf>,
    pub login_standin: Option<PathBuf>,
}
impl FixtureConfiguration {
    pub(super) fn read() -> io::Result<Self> {
        let path = std::env::var_os("QUOTA_EFFECT_FIXTURE_CONFIG")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("QUOTA_EFFECT_FIXTURE_CONFIG is required"))?;
        let config: Self = serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
        config.validate()?;
        Ok(config)
    }
    pub(super) fn validate(&self) -> io::Result<()> {
        let root = &self.output_directory;
        let metadata = fs::symlink_metadata(root)?;
        let canonical = fs::canonicalize(root)?;
        let system_temp = fs::canonicalize(std::env::temp_dir())?;
        let checkout_temp = std::env::current_dir()?.join("tmp");
        if !root.is_absolute()
            || metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
            || (!canonical.starts_with(&system_temp) && !canonical.starts_with(&checkout_temp))
        {
            return Err(io::Error::other(
                "fixture output must be an owned private temporary directory",
            ));
        }
        let mut paths = vec![
            &self.events,
            &self.receipt,
            &self.control_socket,
            &self.dispatch_receipt,
        ];
        if let Some(path) = &self.login_receipt {
            paths.push(path);
        }
        if let Some(path) = &self.login_gate_socket {
            paths.push(path);
        }
        let mut unique = std::collections::HashSet::new();
        for path in paths {
            if !path.is_absolute()
                || path.parent().map(fs::canonicalize).transpose()?.as_ref() != Some(&canonical)
                || fs::symlink_metadata(path).is_ok()
                || !unique.insert(path)
                || path
                    .parent()
                    .map(fs::metadata)
                    .transpose()?
                    .as_ref()
                    .map(MetadataExt::uid)
                    != Some(metadata.uid())
            {
                return Err(io::Error::other(
                    "each fixture output must be distinct, absent and directly inside its private output directory",
                ));
            }
        }
        let login_fields = [
            self.login_receipt.is_some(),
            self.login_gate_socket.is_some(),
            self.login_standin.is_some(),
        ];
        if if self.scenario.requires_login_standin() {
            !login_fields.iter().all(|present| *present)
        } else {
            login_fields.iter().any(|present| *present)
        } {
            return Err(io::Error::other(
                "login and grouping-reopen scenarios require the existing private child guard fields",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum FixtureControlRequest {
    Readback {
        request_id: u64,
    },
    SelectBrowseAccount {
        request_id: u64,
        mount: u64,
        account_id: String,
    },
    ReloadViewModel {
        request_id: u64,
        mount: u64,
    },
    RemoveCapturedAccountExternally {
        request_id: u64,
        mount: u64,
    },
    ReenableBeforeRemoval {
        request_id: u64,
    },
    ReleaseRemoval {
        request_id: u64,
    },
    ReleaseRefresh {
        request_id: u64,
    },
}

pub(super) fn absent_file(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}
