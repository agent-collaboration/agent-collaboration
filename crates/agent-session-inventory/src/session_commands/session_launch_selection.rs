//! Native launch target and profile selection for the Sessions product.

use std::ffi::OsString;
use std::path::PathBuf;

use super::SessionsCommandError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionsLaunchTarget {
    Hosted {
        app_server_socket: PathBuf,
        service_directory: PathBuf,
        codex_home: PathBuf,
        invoking_cwd: PathBuf,
        profile: codex_native_integration::SessionProfile,
    },
    Local {
        invoking_cwd: PathBuf,
        profile: codex_native_integration::SessionProfile,
    },
}

impl SessionsLaunchTarget {
    pub fn resolve_for_selection(
        &mut self,
        identity: &super::SessionPickerIdentity,
    ) -> Result<(), SessionsCommandError> {
        match identity {
            super::SessionPickerIdentity::LocalCodex(_) => self.resolve_for_launch(),
            super::SessionPickerIdentity::HostedCodex(target) => {
                let Self::Hosted {
                    app_server_socket,
                    service_directory,
                    ..
                } = self
                else {
                    return Err(SessionsCommandError::SessionSourceUnavailable);
                };
                *app_server_socket = collaboration_client::resolve_public_native_for_endpoint(
                    service_directory,
                    &target.endpoint,
                )
                .map_err(SessionsCommandError::CodexLaunch)?;
                Ok(())
            }
            super::SessionPickerIdentity::HostedProvider(_)
            | super::SessionPickerIdentity::NativeClaude(_) => {
                Err(SessionsCommandError::SessionSourceUnavailable)
            }
        }
    }

    pub fn resolve_for_launch(&mut self) -> Result<(), SessionsCommandError> {
        if let Self::Hosted {
            app_server_socket,
            service_directory,
            ..
        } = self
        {
            *app_server_socket = collaboration_client::resolve_public_native(service_directory)
                .map_err(SessionsCommandError::CodexLaunch)?;
        }
        Ok(())
    }

    /// Stops a hosted resume or fork when the selected profile sets a permission key,
    /// naming the keys and file instead of leaving the user with Codex's generic
    /// "Permission overrides are not supported when resuming a remote task."
    ///
    /// A missing, unreadable or malformed profile is left to Codex, which reports it.
    pub fn ensure_profile_allows_remote_resume(&self) -> Result<(), SessionsCommandError> {
        let Self::Hosted {
            codex_home,
            profile,
            ..
        } = self
        else {
            return Ok(());
        };
        let profile_path = codex_home.join(profile.file_name());
        let Ok(profile_text) = std::fs::read_to_string(&profile_path) else {
            return Ok(());
        };
        let Ok(keys) =
            codex_native_integration::profile_remote_resume_permission_keys(&profile_text)
        else {
            return Ok(());
        };
        if keys.is_empty() {
            return Ok(());
        }
        Err(SessionsCommandError::ProfileBlocksRemoteResume { profile_path, keys })
    }

    fn profile(&self) -> codex_native_integration::SessionProfile {
        match self {
            Self::Hosted { profile, .. } | Self::Local { profile, .. } => *profile,
        }
    }

    pub fn new_launch(&self, codex_args: &[OsString]) -> codex_native_integration::SessionLaunch {
        match self {
            Self::Hosted {
                app_server_socket,
                invoking_cwd,
                ..
            } => codex_native_integration::SessionLaunch::new(
                app_server_socket,
                invoking_cwd,
                codex_args,
            ),
            Self::Local { invoking_cwd, .. } => {
                codex_native_integration::SessionLaunch::local(invoking_cwd, codex_args)
            }
        }
        .with_profile(self.profile())
    }

    pub fn resume_launch(
        &self,
        codex_args: &[OsString],
        session_id: &str,
        model_choice: &codex_native_integration::ResumeModelChoice,
    ) -> codex_native_integration::SessionLaunch {
        match self {
            Self::Hosted {
                app_server_socket,
                invoking_cwd,
                ..
            } => codex_native_integration::SessionLaunch::resume(
                app_server_socket,
                invoking_cwd,
                codex_args,
                session_id,
                model_choice,
            ),
            Self::Local { invoking_cwd, .. } => {
                codex_native_integration::SessionLaunch::resume_local(
                    invoking_cwd,
                    codex_args,
                    session_id,
                    model_choice,
                )
            }
        }
        .with_profile(self.profile())
    }

    pub fn fork_launch(
        &self,
        codex_args: &[OsString],
        session_id: &str,
        model_choice: &codex_native_integration::ResumeModelChoice,
    ) -> codex_native_integration::SessionLaunch {
        match self {
            Self::Hosted {
                app_server_socket,
                invoking_cwd,
                ..
            } => codex_native_integration::SessionLaunch::fork(
                app_server_socket,
                invoking_cwd,
                codex_args,
                session_id,
                model_choice,
            ),
            Self::Local { invoking_cwd, .. } => {
                codex_native_integration::SessionLaunch::fork_local(
                    invoking_cwd,
                    codex_args,
                    session_id,
                    model_choice,
                )
            }
        }
        .with_profile(self.profile())
    }
}

#[cfg(test)]
#[path = "session_launch_selection_tests.rs"]
mod session_launch_selection_tests;
