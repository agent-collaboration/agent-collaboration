//! Native Claude package export joined only to its actual registered manifests.
#[path = "claude_plugin_components.rs"]
mod components;
#[path = "claude_installed_packages.rs"]
mod installed_packages;
use super::*;
use acp_client_runtime::native_capabilities::read_claude_plugin_export;
use installed_packages::read_installed_packages;
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeMarketplaceRegistration {
    install_location: Option<PathBuf>,
    source: NativeMarketplaceSourceWire,
}
#[derive(Deserialize)]
struct NativeMarketplaceSourceWire {
    source: String,
}

impl NativeMarketplaceRegistration {
    fn manifest_path(&self) -> Result<PathBuf, CatalogueReadFailure> {
        match self.source.source.as_str() {
            "github" | "git" | "file" | "directory" => actual_marketplace_manifest(
                self.install_location
                    .as_deref()
                    .ok_or(CatalogueReadFailure::InvalidScope)?,
            ),
            _ => Err(CatalogueReadFailure::InvalidScope),
        }
    }
}
#[derive(Deserialize)]
struct NativeMarketplaceManifest {
    name: String,
    plugins: Vec<NativeMarketplacePluginEntry>,
}
#[derive(Deserialize)]
struct NativeMarketplacePluginEntry {
    name: String,
    description: Option<String>,
    strict: Option<bool>,
    skills: Option<NativeSkillDirectories>,
    #[serde(flatten)]
    additional: BTreeMap<String, serde::de::IgnoredAny>,
}
#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum NativeSkillDirectories {
    One(String),
    Many(Vec<String>),
}

#[derive(Default, Eq, PartialEq)]
enum NativeManifestOrigin {
    #[default]
    PackageFile,
    MarketplaceEntry,
}

#[derive(Deserialize)]
struct NativePluginManifest {
    name: String,
    description: Option<String>,
    skills: Option<NativeSkillDirectories>,
    #[serde(skip)]
    origin: NativeManifestOrigin,
    #[serde(flatten)]
    additional: BTreeMap<String, serde::de::IgnoredAny>,
}

/// Request-local results keep package facts and their skill-source accounting separate.
pub(crate) struct ClaudePackageRead {
    pub plugins: CatalogueSection<NativePlugin>,
    pub skills: CatalogueSection<NativeSkill>,
}

impl ProviderNativeReadContext {
    pub(crate) async fn claude_plugins(
        &self,
        workspace: PathBuf,
        cancelled: CancellationToken,
    ) -> Result<ClaudePackageRead, CatalogueReadFailure> {
        let executable = self
            .launch
            .environment
            .iter()
            .rev()
            .find(|(name, _)| name == "CLAUDE_CODE_EXECUTABLE")
            .map(|(_, value)| PathBuf::from(value))
            .ok_or(CatalogueReadFailure::InvalidScope)?;
        if !executable.is_absolute() || !workspace.is_absolute() {
            return Err(CatalogueReadFailure::InvalidScope);
        }
        let config_home = self
            .claude_config_home
            .clone()
            .ok_or(CatalogueReadFailure::InvalidScope)?;
        if !config_home.is_absolute() {
            return Err(CatalogueReadFailure::InvalidScope);
        }
        // The adapter applies managed-policy env before choosing its native
        // executable. Until that tier is qualified, it cannot be assumed equal
        // to this Host launch environment. This is an explicit source gap.
        let checked_home = config_home.clone();
        tokio::task::spawn_blocking(move || qualify_native_export_environment(&checked_home))
            .await
            .map_err(|_| CatalogueReadFailure::Unavailable)??;
        let mut version_command = tokio::process::Command::new(&executable);
        version_command
            .arg("--version")
            .envs(self.launch.environment.iter().cloned())
            .current_dir(&workspace)
            .kill_on_drop(true);
        let version = bounded_native_output(&mut version_command, 512, &cancelled).await?;
        if version.as_slice() != b"2.1.310 (Claude Code)\n" {
            return Err(CatalogueReadFailure::Unavailable);
        }
        let mut command = tokio::process::Command::new(executable);
        command
            .args([
                "--setting-sources",
                "user,project,local",
                "plugin",
                "list",
                "--json",
            ])
            .envs(self.launch.environment.iter().cloned())
            .current_dir(&workspace)
            .kill_on_drop(true);
        let output = bounded_native_output(&mut command, 4_000_000, &cancelled).await?;
        let packages = read_claude_plugin_export("2.1.310", &output)
            .map_err(|_| CatalogueReadFailure::InvalidNativeData)?;
        tokio::task::spawn_blocking(move || {
            read_installed_packages(packages, &config_home, &cancelled)
        })
        .await
        .map_err(|_| CatalogueReadFailure::Unavailable)?
    }
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, CatalogueReadFailure> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|_| CatalogueReadFailure::Unavailable)?;
    let metadata = file
        .metadata()
        .map_err(|_| CatalogueReadFailure::Unavailable)?;
    if !metadata.is_file() || metadata.len() > 4_000_000 {
        return Err(CatalogueReadFailure::InvalidNativeData);
    }
    let mut contents = Vec::new();
    file.take(4_000_001)
        .read_to_end(&mut contents)
        .map_err(|_| CatalogueReadFailure::Unavailable)?;
    if contents.len() > 4_000_000 {
        return Err(CatalogueReadFailure::InvalidNativeData);
    }
    serde_json::from_slice(&contents).map_err(|_| CatalogueReadFailure::InvalidNativeData)
}

fn actual_marketplace_manifest(install_location: &Path) -> Result<PathBuf, CatalogueReadFailure> {
    if !install_location.is_absolute() {
        return Err(CatalogueReadFailure::InvalidScope);
    }
    let nested = install_location.join(".claude-plugin/marketplace.json");
    match fs::metadata(&nested) {
        Ok(metadata) if metadata.is_file() => Ok(nested),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            if install_location.is_file() {
                Ok(install_location.to_owned())
            } else {
                Err(CatalogueReadFailure::Unavailable)
            }
        }
        _ => Err(CatalogueReadFailure::Unavailable),
    }
}

#[derive(Deserialize)]
struct NativePolicyEnvironment {
    env: Option<BTreeMap<String, serde::de::IgnoredAny>>,
}

fn qualify_native_export_environment(config_home: &Path) -> Result<(), CatalogueReadFailure> {
    let remote_path = config_home.join("remote-settings.json");
    if remote_path
        .try_exists()
        .map_err(|_| CatalogueReadFailure::Unavailable)?
    {
        let policy: NativePolicyEnvironment = read_json(&remote_path)?;
        if policy.env.is_some_and(|env| !env.is_empty()) {
            return Err(CatalogueReadFailure::InvalidScope);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let root = Path::new("/Library/Managed Preferences");
        match fs::read_dir(root) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|_| CatalogueReadFailure::Unavailable)?;
                    let path = entry.path();
                    if entry.file_name() == "com.anthropic.claudecode.plist"
                        || path.is_dir()
                            && path
                                .join("com.anthropic.claudecode.plist")
                                .try_exists()
                                .map_err(|_| CatalogueReadFailure::Unavailable)?
                    {
                        return Err(CatalogueReadFailure::InvalidScope);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CatalogueReadFailure::Unavailable),
        }
        for path in [
            "/Library/Application Support/ClaudeCode/managed-settings.json",
            "/Library/Application Support/ClaudeCode/managed-settings.d",
        ] {
            if Path::new(path)
                .try_exists()
                .map_err(|_| CatalogueReadFailure::Unavailable)?
            {
                return Err(CatalogueReadFailure::InvalidScope);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(CatalogueReadFailure::InvalidScope)
    }
}

#[cfg(test)]
#[path = "claude_plugins_tests.rs"]
mod tests;
