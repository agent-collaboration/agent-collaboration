//! Binding-owned launch context and request-scoped native filesystem observations.
mod claude_plugins;
mod claude_skills;
mod cursor_executable;
use crate::ExternalProviderLaunch;
use acp_client_runtime::native_capabilities::{
    CursorFileSkillScope, CursorFilesystemContext, CursorFilesystemReadError,
    CursorSkillFileFailureReason, read_cursor_filesystem_skills,
};
use collaboration_protocol::ProviderKind;
use collaboration_service::provider_capabilities::*;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(crate) struct ProviderNativeReadContext {
    pub(crate) launch: ExternalProviderLaunch,
    pub(crate) provider: ProviderKind,
    pub(crate) cwd: Option<PathBuf>,
    home: Option<PathBuf>,
    claude_config_home: Option<PathBuf>,
    claude_source_modes_qualified: bool,
    cursor_executable: Option<cursor_executable::CursorExecutableSource>,
}

impl ProviderNativeReadContext {
    pub(crate) fn capture(provider: ProviderKind, launch: &ExternalProviderLaunch) -> Self {
        let home = launch
            .environment
            .iter()
            .rev()
            .find(|(name, _)| name == "HOME")
            .map(|(_, value)| PathBuf::from(value))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let claude_config_home = launch
            .environment
            .iter()
            .rev()
            .find(|(name, _)| name == "CLAUDE_CONFIG_DIR")
            .map(|(_, value)| PathBuf::from(value))
            .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
            .or_else(|| home.as_ref().map(|home| home.join(".claude")));
        Self {
            launch: launch.clone(),
            provider,
            cwd: std::env::current_dir().ok(),
            home,
            claude_config_home,
            claude_source_modes_qualified: [
                "CLAUDE_CODE_SIMPLE",
                "CLAUDE_CODE_SAFE_MODE",
                "CLAUDE_CODE_RESTRICTED",
                "CLAUDE_CODE_HOST_SKILL_CATALOG",
                "CLAUDE_CODE_USE_COWORK_PLUGINS",
                "CLAUDE_CODE_PROJECT_DIR_NAME",
                "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
            ]
            .into_iter()
            .all(|key| {
                launch
                    .environment
                    .iter()
                    .rev()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value.is_empty())
                    .unwrap_or_else(|| std::env::var_os(key).is_none_or(|value| value.is_empty()))
            }),
            cursor_executable: if provider == ProviderKind::Cursor {
                cursor_executable::CursorExecutableSource::capture(&launch.executable)
            } else {
                None
            },
        }
    }

    pub(crate) fn confirm_launch_source(&mut self) {
        if let Some(source) = self.cursor_executable.as_mut() {
            source.confirm_after_launch();
        }
    }

    pub(crate) fn plugin_read_workspace(&self) -> Result<PathBuf, CatalogueUnsupported> {
        // The selector has no conversation/cwd and the provider socket serves
        // multiple native setups. User installation scope does not supply the
        // missing request context; no existing lookup here disambiguates it.
        Err(if self.provider == ProviderKind::ClaudeCode {
            CatalogueUnsupported::ScopeUnavailable
        } else {
            CatalogueUnsupported::NativeOperationMissing
        })
    }

    pub(crate) fn home_directory(&self) -> Option<&std::path::Path> {
        self.home.as_deref()
    }

    pub(crate) fn default_workspace(&self) -> Result<PathBuf, CatalogueReadFailure> {
        let mut workspace = self.cwd.clone().ok_or_else(|| {
            tracing::warn!(
                phase = "native-default-workspace",
                category = "launch-cwd-unavailable",
                "native catalogue scope qualification failed"
            );
            CatalogueReadFailure::InvalidScope
        })?;
        let mut arguments = self.launch.arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                // This flag skips project .cursor/cli.json in the native config
                // loader. ACP still discovers commands/skills from its root.
                "acp" | "--disable-auto-update" | "--disable-project-configs" => {}
                "--workspace" => {
                    workspace = PathBuf::from(arguments.next().ok_or_else(|| {
                        tracing::warn!(
                            phase = "native-default-workspace",
                            category = "workspace-argument-missing",
                            "native catalogue scope qualification failed"
                        );
                        CatalogueReadFailure::InvalidScope
                    })?);
                    if !workspace.is_absolute() {
                        tracing::warn!(
                            phase = "native-default-workspace",
                            category = "workspace-argument-relative",
                            "native catalogue scope qualification failed"
                        );
                        return Err(CatalogueReadFailure::InvalidScope);
                    }
                }
                argument if argument.starts_with("--workspace=") => {
                    workspace = PathBuf::from(argument.trim_start_matches("--workspace="));
                    if !workspace.is_absolute() {
                        tracing::warn!(
                            phase = "native-default-workspace",
                            category = "workspace-argument-relative",
                            "native catalogue scope qualification failed"
                        );
                        return Err(CatalogueReadFailure::InvalidScope);
                    }
                }
                _ => {
                    tracing::warn!(
                        phase = "native-default-workspace",
                        category = "launch-argument-unqualified",
                        "native catalogue scope qualification failed"
                    );
                    return Err(CatalogueReadFailure::InvalidScope);
                }
            }
        }
        Ok(workspace)
    }

    pub(crate) async fn skills(
        &self,
        requested: PathBuf,
        cancelled: CancellationToken,
    ) -> Result<CatalogueSection<NativeSkill>, CatalogueReadFailure> {
        if self.provider != ProviderKind::Cursor {
            return Err(CatalogueReadFailure::Unavailable);
        }
        let context = CursorFilesystemContext::new(
            self.home
                .clone()
                .ok_or(CatalogueReadFailure::InvalidScope)?,
            self.default_workspace()?,
        )
        .map_err(map_cursor_failure)?;
        // Cursor ACP does not advertise agentInfo/version. Query the executable
        // of this actual launch, never a global command found independently.
        let source = self.cursor_executable.as_ref().ok_or_else(|| {
            tracing::warn!(
                phase = "cursor-catalogue-read",
                category = "source-not-captured",
                "native catalogue source qualification failed"
            );
            CatalogueReadFailure::InvalidScope
        })?;
        let executable = source
            .executable_for_read()
            .ok_or(CatalogueReadFailure::InvalidScope)?;
        let mut command = tokio::process::Command::new(executable);
        command
            .args(["--disable-auto-update", "--version"])
            .envs(self.launch.environment.iter().cloned())
            .current_dir(
                self.cwd
                    .as_ref()
                    .ok_or(CatalogueReadFailure::InvalidScope)?,
            )
            .kill_on_drop(true);
        let output = bounded_native_output(&mut command, 512, &cancelled).await?;
        let version = std::str::from_utf8(&output)
            .map_err(|_| CatalogueReadFailure::InvalidNativeData)?
            .trim()
            .to_owned();
        let catalogue = tokio::task::spawn_blocking(move || {
            read_cursor_filesystem_skills(&context, &version, &requested, &cancelled)
        })
        .await
        .map_err(|_| CatalogueReadFailure::Unavailable)?
        .map_err(map_cursor_failure)?;
        project_cursor_catalogue(catalogue)
    }
}

fn project_cursor_catalogue(
    catalogue: acp_client_runtime::native_capabilities::CursorFilesystemCatalogue,
) -> Result<CatalogueSection<NativeSkill>, CatalogueReadFailure> {
    let errors = catalogue
        .errors
        .into_iter()
        .map(|failure| {
            Ok(CatalogueSourceError {
                source: NativeErrorSource::SkillSource {
                    actual_path: CapabilityPath::new(failure.path)?,
                },
                reason: match failure.reason {
                    CursorSkillFileFailureReason::Unreadable => CatalogueReadFailure::Unavailable,
                    CursorSkillFileFailureReason::InvalidMetadata => {
                        CatalogueReadFailure::InvalidNativeData
                    }
                },
            })
        })
        .collect::<Result<Vec<_>, CatalogueReadFailure>>()?;
    let items = catalogue
        .skills
        .into_iter()
        .map(|skill| {
            let canonical_name = skill.canonical_name;
            // Native references recognize only a whole ASCII identifier; Host
            // also checks the retained replacement list for command collisions.
            let lowering = if !canonical_name.is_empty()
                && canonical_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                NativeSkillLowering::Qualified {
                    canonical_command: NativeCommandName::new(canonical_name.clone())?,
                }
            } else {
                NativeSkillLowering::Unqualified {
                    reason: LoweringUnsupported::UnsupportedInputForm,
                }
            };
            Ok(NativeSkill {
                id: NativeSkillId::new(
                    skill
                        .path
                        .to_str()
                        .ok_or(CatalogueReadFailure::InvalidNativeData)?
                        .to_owned(),
                )?,
                name: CapabilityName::new(canonical_name)?,
                description: skill.description,
                origin: NativeOrigin::Filesystem {
                    actual_path: CapabilityPath::new(skill.path)?,
                },
                native_scope: match skill.scope {
                    CursorFileSkillScope::User => NativeScope::User,
                    CursorFileSkillScope::Workspace => NativeScope::Workspace,
                },
                availability: if skill.user_invocable {
                    NativeAvailability::Available
                } else {
                    NativeAvailability::Disabled
                },
                display_version: None,
                plugin_id: None,
                lowering,
            })
        })
        .collect::<Result<Vec<_>, CatalogueReadFailure>>()?;
    Ok(CatalogueSection::Supported {
        items,
        enumeration: SourceEnumeration::Accounted,
        errors,
    })
}

/// Bound only the existing read/export processes used by this native reader.
async fn bounded_native_output(
    command: &mut tokio::process::Command,
    maximum_bytes: usize,
    cancelled: &CancellationToken,
) -> Result<Vec<u8>, CatalogueReadFailure> {
    use tokio::io::AsyncReadExt;
    if cancelled.is_cancelled() {
        return Err(CatalogueReadFailure::Cancelled);
    }
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| CatalogueReadFailure::Unavailable)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(CatalogueReadFailure::Unavailable)?;
    let bound =
        u64::try_from(maximum_bytes).map_err(|_| CatalogueReadFailure::InvalidNativeData)? + 1;
    let mut output = Vec::new();
    let mut bounded_stdout = stdout.take(bound);
    tokio::select! {
        () = cancelled.cancelled() => return Err(CatalogueReadFailure::Cancelled),
        result = bounded_stdout.read_to_end(&mut output) => { result.map_err(|_| CatalogueReadFailure::Unavailable)?; },
    }
    if output.len() > maximum_bytes {
        return Err(CatalogueReadFailure::InvalidNativeData);
    }
    let status = tokio::select! {
        () = cancelled.cancelled() => return Err(CatalogueReadFailure::Cancelled),
        status = child.wait() => status.map_err(|_| CatalogueReadFailure::Unavailable)?,
    };
    if !status.success() {
        return Err(CatalogueReadFailure::Unavailable);
    }
    Ok(output)
}

fn map_cursor_failure(error: CursorFilesystemReadError) -> CatalogueReadFailure {
    match error {
        CursorFilesystemReadError::InvalidScope => CatalogueReadFailure::InvalidScope,
        CursorFilesystemReadError::UnsupportedVersion => CatalogueReadFailure::Unavailable,
        CursorFilesystemReadError::Unreadable { .. } => CatalogueReadFailure::Unavailable,
        CursorFilesystemReadError::InvalidMetadata { .. } => {
            CatalogueReadFailure::InvalidNativeData
        }
        CursorFilesystemReadError::AmbiguousOrigin { .. } => {
            CatalogueReadFailure::AmbiguousSelector
        }
        CursorFilesystemReadError::Cancelled => CatalogueReadFailure::Cancelled,
    }
}

#[cfg(test)]
#[path = "provider_native_capabilities_tests.rs"]
mod tests;
