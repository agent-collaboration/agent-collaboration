//! Native Claude package export joined only to its actual registered manifests.
#[path = "claude_plugin_components.rs"]
mod components;
use super::*;
use acp_client_runtime::native_capabilities::{
    ClaudePluginExportScope, read_claude_plugin_export, regular_claude_plugin_key,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

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
    strict: Option<bool>,
    skills: Option<serde::de::IgnoredAny>,
    #[serde(flatten)]
    additional: BTreeMap<String, serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum NativeSkillDirectories {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct NativePluginManifest {
    name: String,
    description: Option<String>,
    skills: Option<NativeSkillDirectories>,
    #[serde(flatten)]
    additional: BTreeMap<String, serde::de::IgnoredAny>,
}

impl ProviderNativeReadContext {
    pub(crate) async fn claude_plugins(
        &self,
        workspace: PathBuf,
        cancelled: CancellationToken,
    ) -> Result<Vec<NativePlugin>, CatalogueReadFailure> {
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
            if packages.is_empty() {
                return Ok(Vec::new());
            }
            let registry_path = config_home.join("plugins/known_marketplaces.json");
            let registered: BTreeMap<String, NativeMarketplaceRegistration> =
                read_json(&registry_path)?;
            let mut seen = HashSet::new();
            let mut plugins = Vec::new();
            for package in packages {
                if cancelled.is_cancelled() {
                    return Err(CatalogueReadFailure::Cancelled);
                }
                if !seen.insert(package.id.clone()) {
                    return Err(CatalogueReadFailure::AmbiguousSelector);
                }
                if !package.errors.is_empty() || !package.notes.is_empty() {
                    return Err(CatalogueReadFailure::InvalidNativeData);
                }
                let (name, marketplace_name) = regular_claude_plugin_key(&package.id)
                    .map_err(|_| CatalogueReadFailure::InvalidNativeData)?;
                let registration = registered
                    .get(marketplace_name)
                    .ok_or(CatalogueReadFailure::InvalidScope)?;
                let marketplace_path = registration.manifest_path()?;
                let marketplace: NativeMarketplaceManifest = read_json(&marketplace_path)?;
                if marketplace.name != marketplace_name {
                    return Err(CatalogueReadFailure::InvalidNativeData);
                }
                if package.scope != ClaudePluginExportScope::User || package.project_path.is_some()
                {
                    return Err(CatalogueReadFailure::InvalidScope);
                }
                let path = package
                    .read_from_folder
                    .as_ref()
                    .unwrap_or(&package.install_path);
                if !path.is_absolute() || !path.is_dir() {
                    return Err(CatalogueReadFailure::InvalidNativeData);
                }
                let manifest: NativePluginManifest =
                    read_json(&path.join(".claude-plugin/plugin.json"))?;
                if manifest.name != name {
                    return Err(CatalogueReadFailure::InvalidNativeData);
                }
                let version = package.folder_version.as_ref().unwrap_or(&package.version);
                let components = components::read_plugin_components(
                    path,
                    &manifest,
                    &marketplace,
                    &package,
                    &cancelled,
                );
                plugins.push(NativePlugin {
                    id: NativePluginId::new(package.id.clone())?,
                    name: CapabilityName::new(manifest.name)?,
                    description: manifest.description,
                    origin: NativeOrigin::Filesystem {
                        actual_path: CapabilityPath::new(path.clone())?,
                    },
                    state: if package.enabled {
                        NativePluginState::InstalledEnabled {
                            loading: NativeLoadingState::NotObserved,
                        }
                    } else {
                        NativePluginState::InstalledDisabled
                    },
                    display_version: if version == "unknown" {
                        None
                    } else {
                        Some(CapabilityName::new(version.clone())?)
                    },
                    marketplace: NativeMarketplaceObservation::Qualified {
                        native_name: CapabilityName::new(marketplace.name)?,
                        source: NativeMarketplaceSource::Filesystem {
                            manifest_path: CapabilityPath::new(marketplace_path)?,
                        },
                    },
                    components,
                    installation_policy: NativePolicyObservation::Undeclared,
                    authentication_policy: NativePolicyObservation::Undeclared,
                });
            }
            Ok(plugins)
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
mod tests {
    use super::components::qualified_plugin_skill_components;
    use super::*;

    #[test]
    fn native_capabilities_marketplace_source_uses_native_manifest_resolution_order() {
        let root = tempfile::tempdir_in("/tmp").expect("fixture");
        let nested = root.path().join(".claude-plugin/marketplace.json");
        fs::create_dir_all(nested.parent().expect("parent")).expect("manifest folder");
        fs::write(&nested, r#"{"name":"native-marketplace"}"#).expect("manifest");
        assert_eq!(actual_marketplace_manifest(root.path()), Ok(nested.clone()));
        assert_eq!(actual_marketplace_manifest(&nested), Ok(nested));
        assert_eq!(
            actual_marketplace_manifest(Path::new("relative")),
            Err(CatalogueReadFailure::InvalidScope)
        );
    }

    #[test]
    fn native_capabilities_named_catalog_cannot_be_reclassified_from_an_install_location() {
        let registration = r#"{"installLocation":"/native/cache","source":{"source":"claudeai"}}"#;
        let registered = serde_json::from_str::<NativeMarketplaceRegistration>(registration)
            .expect("named source");
        assert_eq!(
            registered.manifest_path(),
            Err(CatalogueReadFailure::InvalidScope)
        );
        let registration = r#"{"installLocation":"/native/market","source":{"source":"git"}}"#;
        assert!(serde_json::from_str::<NativeMarketplaceRegistration>(registration).is_ok());
    }
    #[test]
    fn native_capabilities_plugin_component_skill_uses_actual_manifest_roots_and_native_identity() {
        let root = tempfile::tempdir_in("/tmp").expect("fixture");
        fs::create_dir_all(root.path().join("skills/alpha")).expect("skill folder");
        fs::write(
            root.path().join("skills/alpha/SKILL.md"),
            "---\nname: Display title\nuser-invocable: false\n---\n# Native alpha",
        )
        .expect("native skill");
        let manifest: NativePluginManifest = serde_json::from_value(serde_json::json!({
            "name":"native-package","skills":["./skills"]
        }))
        .expect("plugin manifest");
        let marketplace: NativeMarketplaceManifest = serde_json::from_value(serde_json::json!({
            "name":"native-marketplace","plugins":[{"name":"native-package"}]
        }))
        .expect("marketplace manifest");
        let export = serde_json::to_vec(&serde_json::json!([{
            "id":"native-package@native-marketplace","version":"1.0","scope":"user", "enabled":true,"installPath":root.path()
        }])).expect("native export");
        let packages = read_claude_plugin_export("2.1.310", &export).expect("export");
        let (skills, _) = qualified_plugin_skill_components(
            root.path(),
            &manifest,
            &marketplace,
            &packages[0],
            &CancellationToken::new(),
        )
        .expect("qualified skill sources");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name.as_str(), "native-package:alpha");
        assert_eq!(skills[0].availability, NativeAvailability::Disabled);
        assert_eq!(
            skills[0].plugin_id.as_ref().expect("plugin id").as_str(),
            "native-package@native-marketplace"
        );
        assert!(matches!(
            skills[0].lowering,
            NativeSkillLowering::Unqualified { .. }
        ));
        let components = components::read_plugin_components(
            root.path(),
            &manifest,
            &marketplace,
            &packages[0],
            &CancellationToken::new(),
        );
        assert!(
            matches!(components.commands, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
        );
        assert!(
            matches!(components.hooks, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
        );
        assert!(
            matches!(components.mcp, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
        );
        assert!(
            matches!(components.other, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
        );
        fs::create_dir(root.path().join("hooks")).expect("hook folder");
        fs::write(root.path().join("hooks/hooks.json"), "{}").expect("present native source");
        let components = components::read_plugin_components(
            root.path(),
            &manifest,
            &marketplace,
            &packages[0],
            &CancellationToken::new(),
        );
        assert!(matches!(
            components.hooks,
            NativeComponentSection::NotObserved
        ));
        let mut entry_override = marketplace;
        entry_override.plugins[0].skills = Some(serde::de::IgnoredAny);
        assert_eq!(
            qualified_plugin_skill_components(
                root.path(),
                &manifest,
                &entry_override,
                &packages[0],
                &CancellationToken::new()
            ),
            Err(CatalogueReadFailure::InvalidScope)
        );
    }
}
