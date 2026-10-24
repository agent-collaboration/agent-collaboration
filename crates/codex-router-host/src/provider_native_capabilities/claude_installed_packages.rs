//! Account for each native installed package without erasing independent facts.
use super::*;
use acp_client_runtime::native_capabilities::{
    ClaudePluginExport, ClaudePluginExportScope, regular_claude_plugin_key,
};
use std::collections::HashSet;

pub(super) fn read_installed_packages(
    packages: Vec<ClaudePluginExport>,
    config_home: &Path,
    cancelled: &CancellationToken,
) -> Result<ClaudePackageRead, CatalogueReadFailure> {
    if packages.is_empty() {
        return Ok(ClaudePackageRead {
            plugins: CatalogueSection::Supported {
                items: Vec::new(),
                enumeration: SourceEnumeration::Accounted,
                errors: Vec::new(),
            },
            skills: empty_skill_sources(),
        });
    }
    let registry_path = config_home.join("plugins/known_marketplaces.json");
    let registered: BTreeMap<String, NativeMarketplaceRegistration> =
        match read_json(&registry_path) {
            Ok(registered) => registered,
            Err(reason) => {
                let error = other_error(&registry_path, reason);
                let mut skills = empty_skill_sources();
                append_skill_sources(&mut skills, unqualified_skill_sources(error.clone()));
                return Ok(ClaudePackageRead {
                    plugins: CatalogueSection::Supported {
                        items: Vec::new(),
                        errors: vec![error],
                        enumeration: SourceEnumeration::Incomplete {
                            limitations: CatalogueLimitations::new(
                                CatalogueLimitation::MissingOrigin,
                                Vec::new(),
                            ),
                        },
                    },
                    skills,
                });
            }
        };
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    let mut errors = Vec::new();
    let mut skills = empty_skill_sources();
    for package in packages {
        if cancelled.is_cancelled() {
            return Err(CatalogueReadFailure::Cancelled);
        }
        let result = if seen.insert(package.id.clone()) {
            read_installed_package(&package, &registered, cancelled)
        } else {
            Err(unlocated_error(CatalogueReadFailure::AmbiguousSelector))
        };
        match result {
            Ok((package, sources)) => {
                items.push(package);
                append_skill_sources(&mut skills, sources);
            }
            Err(error) if !errors.contains(&error) => {
                append_skill_sources(&mut skills, unqualified_skill_sources(error.clone()));
                errors.push(error);
            }
            Err(_) => {}
        }
    }
    let enumeration = if errors
        .iter()
        .any(|error| error.source == NativeErrorSource::Unobserved)
    {
        SourceEnumeration::Incomplete {
            limitations: CatalogueLimitations::new(
                CatalogueLimitation::PartialNativeExport,
                Vec::new(),
            ),
        }
    } else {
        SourceEnumeration::Accounted
    };
    Ok(ClaudePackageRead {
        plugins: CatalogueSection::Supported {
            items,
            enumeration,
            errors,
        },
        skills,
    })
}

fn read_installed_package(
    package: &ClaudePluginExport,
    registered: &BTreeMap<String, NativeMarketplaceRegistration>,
    cancelled: &CancellationToken,
) -> Result<(NativePlugin, CatalogueSection<NativeSkill>), CatalogueSourceError> {
    if !package.errors.is_empty() || !package.notes.is_empty() {
        return Err(unlocated_error(CatalogueReadFailure::InvalidNativeData));
    }
    let (name, marketplace_name) = regular_claude_plugin_key(&package.id)
        .map_err(|_| unlocated_error(CatalogueReadFailure::InvalidNativeData))?;
    if package.scope != ClaudePluginExportScope::User || package.project_path.is_some() {
        return Err(unlocated_error(CatalogueReadFailure::InvalidScope));
    }
    let path = package
        .read_from_folder
        .as_ref()
        .unwrap_or(&package.install_path);
    if !path.is_absolute() || !path.is_dir() {
        return Err(unlocated_error(CatalogueReadFailure::InvalidNativeData));
    }
    let registration = registered
        .get(marketplace_name)
        .ok_or_else(|| unlocated_error(CatalogueReadFailure::InvalidScope))?;
    let marketplace_path = registration.manifest_path().map_err(unlocated_error)?;
    let marketplace: NativeMarketplaceManifest = read_json(&marketplace_path)
        .map_err(|reason| marketplace_error(&marketplace_path, reason))?;
    if marketplace.name != marketplace_name {
        return Err(marketplace_error(
            &marketplace_path,
            CatalogueReadFailure::InvalidNativeData,
        ));
    }
    let manifest = read_effective_manifest(path, name, &marketplace, &marketplace_path)?;
    if manifest.name != name {
        return Err(match manifest.origin {
            NativeManifestOrigin::PackageFile => other_error(
                &path.join(".claude-plugin/plugin.json"),
                CatalogueReadFailure::InvalidNativeData,
            ),
            NativeManifestOrigin::MarketplaceEntry => {
                marketplace_error(&marketplace_path, CatalogueReadFailure::InvalidNativeData)
            }
        });
    }
    let version = package.folder_version.as_ref().unwrap_or(&package.version);
    let (components, skills) =
        components::read_plugin_components(path, &manifest, &marketplace, package, cancelled);
    Ok((
        NativePlugin {
            id: NativePluginId::new(package.id.clone()).map_err(unlocated_error)?,
            name: CapabilityName::new(manifest.name).map_err(unlocated_error)?,
            description: manifest.description,
            origin: NativeOrigin::Filesystem {
                actual_path: CapabilityPath::new(path.clone()).map_err(unlocated_error)?,
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
                Some(CapabilityName::new(version.clone()).map_err(unlocated_error)?)
            },
            marketplace: NativeMarketplaceObservation::Qualified {
                native_name: CapabilityName::new(marketplace.name).map_err(unlocated_error)?,
                source: NativeMarketplaceSource::Filesystem {
                    manifest_path: CapabilityPath::new(marketplace_path)
                        .map_err(unlocated_error)?,
                },
            },
            components,
            installation_policy: NativePolicyObservation::Undeclared,
            authentication_policy: NativePolicyObservation::Undeclared,
        },
        skills,
    ))
}

fn empty_skill_sources() -> CatalogueSection<NativeSkill> {
    CatalogueSection::Supported {
        items: Vec::new(),
        errors: Vec::new(),
        enumeration: SourceEnumeration::Accounted,
    }
}

fn unqualified_skill_sources(error: CatalogueSourceError) -> CatalogueSection<NativeSkill> {
    CatalogueSection::Supported {
        items: Vec::new(),
        errors: vec![error],
        enumeration: SourceEnumeration::Incomplete {
            limitations: CatalogueLimitations::new(
                CatalogueLimitation::PartialNativeExport,
                Vec::new(),
            ),
        },
    }
}

fn append_skill_sources(
    target: &mut CatalogueSection<NativeSkill>,
    source: CatalogueSection<NativeSkill>,
) {
    let CatalogueSection::Supported {
        items,
        errors,
        enumeration,
    } = target
    else {
        return;
    };
    match source {
        CatalogueSection::Supported {
            items: source_items,
            errors: source_errors,
            enumeration: source_enumeration,
        } => {
            items.extend(source_items);
            for error in source_errors {
                if !errors.contains(&error) {
                    errors.push(error);
                }
            }
            if let SourceEnumeration::Incomplete { limitations } = source_enumeration {
                *enumeration = SourceEnumeration::Incomplete { limitations };
            }
        }
        CatalogueSection::Unsupported { .. } => {
            *enumeration = SourceEnumeration::Incomplete {
                limitations: CatalogueLimitations::new(
                    CatalogueLimitation::PartialNativeExport,
                    Vec::new(),
                ),
            };
        }
    }
}

fn read_effective_manifest(
    package_path: &Path,
    native_name: &str,
    marketplace: &NativeMarketplaceManifest,
    marketplace_path: &Path,
) -> Result<NativePluginManifest, CatalogueSourceError> {
    let mut entries = marketplace
        .plugins
        .iter()
        .filter(|entry| entry.name == native_name);
    let entry = entries.next();
    if entries.next().is_some() {
        return Err(marketplace_error(
            marketplace_path,
            CatalogueReadFailure::AmbiguousSelector,
        ));
    }
    let manifest_path = package_path.join(".claude-plugin/plugin.json");
    match fs::metadata(&manifest_path) {
        Ok(_) => {
            // Native tJ skips only ENOENT/ENOTDIR. A present corrupt file is
            // a located error, never permission to use marketplace fallback.
            let mut manifest: NativePluginManifest =
                read_json(&manifest_path).map_err(|reason| other_error(&manifest_path, reason))?;
            if let Some(description) = entry
                .and_then(|entry| entry.description.as_ref())
                .filter(|description| !description.trim().is_empty())
            {
                manifest.description = Some(description.clone());
            }
            Ok(manifest)
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            let entry = entry.ok_or_else(|| {
                marketplace_error(marketplace_path, CatalogueReadFailure::NotFound)
            })?;
            // Native y9n assigns the actual entry when tJ reports no manifest.
            Ok(NativePluginManifest {
                name: entry.name.clone(),
                description: entry.description.clone(),
                skills: entry.skills.clone(),
                origin: NativeManifestOrigin::MarketplaceEntry,
                additional: entry.additional.clone(),
            })
        }
        Err(_) => Err(other_error(
            &manifest_path,
            CatalogueReadFailure::Unavailable,
        )),
    }
}

fn unlocated_error(reason: CatalogueReadFailure) -> CatalogueSourceError {
    CatalogueSourceError {
        source: NativeErrorSource::Unobserved,
        reason,
    }
}

fn other_error(path: &Path, reason: CatalogueReadFailure) -> CatalogueSourceError {
    CatalogueSourceError {
        source: CapabilityPath::new(path.to_owned())
            .map_or(NativeErrorSource::Unobserved, |actual_path| {
                NativeErrorSource::OtherFilesystem { actual_path }
            }),
        reason,
    }
}

fn marketplace_error(path: &Path, reason: CatalogueReadFailure) -> CatalogueSourceError {
    CatalogueSourceError {
        source: CapabilityPath::new(path.to_owned())
            .map_or(NativeErrorSource::Unobserved, |actual_path| {
                NativeErrorSource::MarketplaceManifest { actual_path }
            }),
        reason,
    }
}
