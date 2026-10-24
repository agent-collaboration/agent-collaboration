//! Per-kind native component coverage; absent exports alone never mean empty.
use super::*;
use std::collections::HashSet;

pub(super) fn read_plugin_components(
    path: &Path,
    manifest: &NativePluginManifest,
    marketplace: &NativeMarketplaceManifest,
    package: &acp_client_runtime::native_capabilities::ClaudePluginExport,
    cancelled: &CancellationToken,
) -> (NativePluginComponents, CatalogueSection<NativeSkill>) {
    let (skill_sources, has_skill_hooks) =
        match qualified_plugin_skill_components(path, manifest, marketplace, package, cancelled) {
            Ok((sources, hooks)) => (sources, Some(hooks)),
            Err(_) => (
                CatalogueSection::Supported {
                    items: Vec::new(),
                    errors: Vec::new(),
                    enumeration: SourceEnumeration::Incomplete {
                        limitations: CatalogueLimitations::new(
                            CatalogueLimitation::PartialNativeExport,
                            Vec::new(),
                        ),
                    },
                },
                None,
            ),
        };
    let skills = match &skill_sources {
        CatalogueSection::Supported {
            items,
            enumeration: SourceEnumeration::Accounted,
            errors,
        } => NativeComponentSection::Observed {
            items: items.clone(),
            coverage: if errors.is_empty() {
                CatalogueCoverage::Complete
            } else {
                CatalogueCoverage::Partial
            },
        },
        _ => NativeComponentSection::NotObserved,
    };
    let source_entry = marketplace
        .plugins
        .iter()
        .filter(|entry| entry.name == manifest.name)
        .collect::<Vec<_>>();
    let declared_by_entry = |keys: &[&str]| match source_entry.as_slice() {
        [entry]
            if entry.strict != Some(false)
                || manifest.origin == NativeManifestOrigin::MarketplaceEntry =>
        {
            keys.iter().any(|key| entry.additional.contains_key(*key))
        }
        _ => true,
    };
    // Kle's conventional component roots and manifest keys are independent.
    // Any declaration or present conventional source stays unobserved until its
    // semantics are qualified; only proven absence earns complete empty.
    let commands = if declared_by_entry(&["commands"]) {
        NativeComponentSection::NotObserved
    } else {
        empty_if_absent(path, manifest, &["commands"], &["commands"])
    };
    let mcp = if declared_by_entry(&["mcpServers"])
        || package
            .mcp_servers
            .as_ref()
            .is_some_and(|servers| !servers.is_empty())
    {
        NativeComponentSection::NotObserved
    } else {
        empty_if_absent(path, manifest, &["mcpServers"], &[".mcp.json"])
    };
    let hooks = if has_skill_hooks == Some(false) && !declared_by_entry(&["hooks"]) {
        empty_if_absent(path, manifest, &["hooks"], &["hooks/hooks.json"])
    } else {
        NativeComponentSection::NotObserved
    };
    let other_keys = [
        "agents",
        "outputStyles",
        "themes",
        "workflows",
        "monitors",
        "lspServers",
        "settings",
        "experimental",
    ];
    let other = if declared_by_entry(&other_keys) {
        NativeComponentSection::NotObserved
    } else {
        empty_if_absent(
            path,
            manifest,
            &[
                "agents",
                "outputStyles",
                "themes",
                "workflows",
                "monitors",
                "lspServers",
                "settings",
                "experimental",
            ],
            &[
                "agents",
                "output-styles",
                "themes",
                "workflows",
                "monitors/monitors.json",
                ".lsp.json",
                "settings.json",
            ],
        )
    };
    (
        NativePluginComponents {
            skills,
            commands,
            mcp,
            hooks,
            other,
        },
        skill_sources,
    )
}

fn empty_if_absent<T>(
    path: &Path,
    manifest: &NativePluginManifest,
    keys: &[&str],
    defaults: &[&str],
) -> NativeComponentSection<T> {
    if keys
        .iter()
        .any(|key| manifest.additional.contains_key(*key))
        || defaults
            .iter()
            .any(|relative| !matches!(path.join(relative).try_exists(), Ok(false)))
    {
        return NativeComponentSection::NotObserved;
    }
    NativeComponentSection::Observed {
        items: Vec::new(),
        coverage: CatalogueCoverage::Complete,
    }
}

pub(super) fn qualified_plugin_skill_components(
    path: &Path,
    manifest: &NativePluginManifest,
    marketplace: &NativeMarketplaceManifest,
    package: &acp_client_runtime::native_capabilities::ClaudePluginExport,
    cancelled: &CancellationToken,
) -> Result<(CatalogueSection<NativeSkill>, bool), CatalogueReadFailure> {
    let entries = marketplace
        .plugins
        .iter()
        .filter(|entry| entry.name == manifest.name)
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        return Err(CatalogueReadFailure::AmbiguousSelector);
    };
    if entry.skills.is_some()
        || entry.strict == Some(false) && manifest.origin != NativeManifestOrigin::MarketplaceEntry
    {
        return Err(CatalogueReadFailure::InvalidScope);
    }
    let mut roots = Vec::new();
    let default = path.join("skills");
    if default
        .try_exists()
        .map_err(|_| CatalogueReadFailure::Unavailable)?
    {
        roots.push(default);
    }
    match &manifest.skills {
        Some(paths) => {
            let declared = match paths {
                NativeSkillDirectories::One(path) => vec![path],
                NativeSkillDirectories::Many(paths) => paths.iter().collect(),
            };
            for declared in declared {
                let relative = Path::new(declared);
                if relative.is_absolute()
                    || relative
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    return Err(CatalogueReadFailure::InvalidScope);
                }
                let resolved = path.join(relative);
                if !resolved.is_dir() {
                    return Err(CatalogueReadFailure::InvalidNativeData);
                }
                roots.push(resolved);
            }
        }
        None if roots.is_empty()
            && path
                .join("SKILL.md")
                .try_exists()
                .map_err(|_| CatalogueReadFailure::Unavailable)? =>
        {
            roots.push(path.to_owned())
        }
        None => {}
    }
    let real_plugin = fs::canonicalize(path).map_err(|_| CatalogueReadFailure::Unavailable)?;
    let mut seen = HashSet::new();
    let mut distinct = Vec::new();
    for root in roots {
        let real_root = fs::canonicalize(&root).map_err(|_| CatalogueReadFailure::Unavailable)?;
        if !real_root.starts_with(&real_plugin) {
            return Err(CatalogueReadFailure::InvalidScope);
        }
        if seen.insert(real_root) {
            distinct.push(root);
        }
    }
    let skills = acp_client_runtime::native_capabilities::read_claude_plugin_skill_roots(
        "2.1.310",
        &manifest.name,
        &distinct,
        acp_client_runtime::native_capabilities::ClaudeFileSkillScope::User,
        package.enabled,
        cancelled,
    )
    .map_err(|_| CatalogueReadFailure::InvalidNativeData)?;
    // Failed files cannot establish hook absence, even alongside healthy files.
    let hook_absence_unproven = !skills.errors.is_empty()
        || skills
            .skills
            .iter()
            .any(|skill| skill.has_hook_declarations);
    let errors = skills.errors.into_iter().map(|failure| Ok(CatalogueSourceError {
        source: NativeErrorSource::SkillSource { actual_path: CapabilityPath::new(failure.path)? },
        reason: match failure.reason {
            acp_client_runtime::native_capabilities::ClaudeFilesystemReadError::Unreadable => CatalogueReadFailure::Unavailable,
            _ => CatalogueReadFailure::InvalidNativeData,
        },
    })).collect::<Result<Vec<_>, CatalogueReadFailure>>()?;
    let items = skills
        .skills
        .into_iter()
        .map(|skill| {
            let lowering = super::super::claude_skills::qualified_claude_command(
                &skill.canonical_name,
                skill.native_display_name.as_deref(),
            );
            Ok(NativeSkill {
                id: NativeSkillId::new(
                    skill
                        .path
                        .to_str()
                        .ok_or(CatalogueReadFailure::InvalidNativeData)?
                        .to_owned(),
                )?,
                name: CapabilityName::new(skill.canonical_name)?,
                description: skill.description,
                origin: NativeOrigin::Filesystem {
                    actual_path: CapabilityPath::new(skill.path)?,
                },
                native_scope: NativeScope::User,
                availability: if skill.user_invocable {
                    NativeAvailability::Available
                } else {
                    NativeAvailability::Disabled
                },
                display_version: None,
                plugin_id: Some(NativePluginId::new(package.id.clone())?),
                lowering,
            })
        })
        .collect::<Result<Vec<_>, CatalogueReadFailure>>()?;
    Ok((
        CatalogueSection::Supported {
            items,
            errors,
            enumeration: SourceEnumeration::Accounted,
        },
        hook_absence_unproven,
    ))
}
