//! Compose the native ordinary, legacy and installed-plugin filesystem views.
#[path = "claude_project_context.rs"]
mod project_context;
#[cfg(test)]
#[path = "claude_project_roots_tests.rs"]
mod project_roots_tests;
use super::*;
use acp_client_runtime::native_capabilities::{
    ClaudeFileSkill, ClaudeFileSkillScope, ClaudeSkillOverride, ClaudeSkillRoot,
    read_claude_filesystem_skills, read_claude_legacy_skill_files,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeSkillSettings {
    #[serde(default)]
    skill_overrides: HashMap<String, ClaudeSkillOverride>,
    strict_plugin_only_customization: Option<NativeStrictCustomization>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NativeStrictCustomization {
    All(bool),
    Kinds(Vec<String>),
}

impl NativeSkillSettings {
    fn only_plugin_skills(&self) -> bool {
        match &self.strict_plugin_only_customization {
            Some(NativeStrictCustomization::All(all)) => *all,
            Some(NativeStrictCustomization::Kinds(kinds)) => {
                kinds.iter().any(|kind| kind == "skills")
            }
            None => false,
        }
    }
}

impl ProviderNativeReadContext {
    pub(crate) async fn claude_skill_section(
        &self,
        workspace: PathBuf,
        plugin_skills: CatalogueSection<NativeSkill>,
        cancelled: CancellationToken,
    ) -> Result<CatalogueSection<NativeSkill>, CatalogueReadFailure> {
        if !self.claude_source_modes_qualified {
            return Ok(incomplete_skills(
                Vec::new(),
                Vec::new(),
                CatalogueLimitation::MissingScope,
            ));
        }
        let home = self
            .home
            .clone()
            .ok_or(CatalogueReadFailure::InvalidScope)?;
        let config_home = self
            .claude_config_home
            .clone()
            .ok_or(CatalogueReadFailure::InvalidScope)?;
        tokio::task::spawn_blocking(move || {
            read_native_claude_skill_section(
                &home,
                &config_home,
                &workspace,
                plugin_skills,
                &cancelled,
            )
        })
        .await
        .map_err(|_| CatalogueReadFailure::Unavailable)?
    }
}

fn read_native_claude_skill_section(
    home: &Path,
    config_home: &Path,
    workspace: &Path,
    plugin_skills: CatalogueSection<NativeSkill>,
    cancelled: &CancellationToken,
) -> Result<CatalogueSection<NativeSkill>, CatalogueReadFailure> {
    if cancelled.is_cancelled() {
        return Err(CatalogueReadFailure::Cancelled);
    }
    if !home.is_absolute() || !config_home.is_absolute() || !workspace.is_absolute() {
        return Err(CatalogueReadFailure::InvalidScope);
    }
    let home = fs::canonicalize(home).map_err(|_| CatalogueReadFailure::InvalidScope)?;
    let workspace = fs::canonicalize(workspace).map_err(|_| CatalogueReadFailure::InvalidScope)?;
    let project = project_context::read_project_context(&home, &workspace)?;
    // Current native ICr/UEo consult the managed directory separately from
    // settings. Presence needs its own ownership/policy qualification.
    let managed = Path::new("/Library/Application Support/ClaudeCode/.claude");
    for kind in ["skills", "commands"] {
        if managed
            .join(kind)
            .try_exists()
            .map_err(|_| CatalogueReadFailure::Unavailable)?
        {
            return Ok(incomplete_skills(
                Vec::new(),
                Vec::new(),
                CatalogueLimitation::MissingScope,
            ));
        }
    }
    let policy = read_settings_if_present(&config_home.join("remote-settings.json"))?;
    let mut overrides =
        read_settings_if_present(&config_home.join("settings.json"))?.skill_overrides;
    // Native projectSettings is scoped to the session cwd, unlike Zte's
    // ancestor skill directories. Local settings may canonicalize to git root;
    // the currently qualified setup below excludes nested/worktree cases.
    overrides.extend(
        read_settings_if_present(&workspace.join(".claude/settings.json"))?.skill_overrides,
    );
    for local in project.local_settings_files {
        overrides.extend(read_settings_if_present(&local)?.skill_overrides);
    }
    // Administrator overrides are applied after user/project/local values.
    overrides.extend(policy.skill_overrides.clone());
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    let mut limitation = None;
    if !policy.only_plugin_skills() {
        let synced = config_home.join("skills/synced");
        match fs::read_dir(&synced) {
            Ok(mut entries) => {
                if entries
                    .next()
                    .transpose()
                    .map_err(|_| CatalogueReadFailure::Unavailable)?
                    .is_some()
                {
                    limitation = Some(CatalogueLimitation::MissingOrigin);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CatalogueReadFailure::Unavailable),
        }
        let mut roots = vec![ClaudeSkillRoot {
            directory: config_home.join("skills"),
            scope: ClaudeFileSkillScope::User,
        }];
        roots.extend(
            project
                .skill_directories
                .iter()
                .map(|directory| ClaudeSkillRoot {
                    directory: directory.clone(),
                    scope: ClaudeFileSkillScope::Workspace,
                }),
        );
        let ordinary = read_claude_filesystem_skills("2.1.310", &roots, &overrides, cancelled)
            .map_err(map_claude_file_failure)?;
        for failure in ordinary.errors {
            errors.push(CatalogueSourceError {
                source: NativeErrorSource::SkillSource {
                    actual_path: CapabilityPath::new(failure.path)?,
                },
                reason: map_claude_file_failure(failure.reason),
            });
        }
        for skill in ordinary.skills {
            skills.push(project_claude_file_skill(skill)?);
        }
        let mut command_roots = vec![ClaudeSkillRoot {
            directory: config_home.join("commands"),
            scope: ClaudeFileSkillScope::User,
        }];
        command_roots.extend(
            project
                .command_directories
                .iter()
                .map(|directory| ClaudeSkillRoot {
                    directory: directory.clone(),
                    scope: ClaudeFileSkillScope::Workspace,
                }),
        );
        let legacy =
            read_claude_legacy_skill_files("2.1.310", &command_roots, &overrides, cancelled)
                .map_err(map_claude_file_failure)?;
        for failure in legacy.errors {
            errors.push(CatalogueSourceError {
                source: NativeErrorSource::SkillSource {
                    actual_path: CapabilityPath::new(failure.path)?,
                },
                reason: map_claude_file_failure(failure.reason),
            });
        }
        for skill in legacy.skills {
            skills.push(project_claude_file_skill(skill)?);
        }
    }
    match plugin_skills {
        CatalogueSection::Supported {
            items,
            errors: plugin_errors,
            enumeration,
        } => {
            errors.extend(plugin_errors);
            if matches!(enumeration, SourceEnumeration::Incomplete { .. }) {
                limitation = Some(CatalogueLimitation::PartialNativeExport);
            }
            // Regular installed plugins use native eke's plugin branch. Their
            // package enablement and author invocation restriction were already
            // observed; ordinary skillOverrides do not govern these records.
            skills.extend(items);
        }
        CatalogueSection::Unsupported { .. } => {
            limitation = Some(CatalogueLimitation::PartialNativeExport)
        }
    }
    // Native ICr realpath dedup keeps the first source, without inventing a
    // second identity for a symlink to that same physical skill file.
    let mut seen_origins = HashSet::new();
    let mut distinct = Vec::new();
    for skill in skills {
        if let NativeOrigin::Filesystem { actual_path } = &skill.origin {
            match fs::canonicalize(actual_path.as_path()) {
                Ok(real) => {
                    if seen_origins.insert(real) {
                        distinct.push(skill);
                    }
                }
                Err(_) => errors.push(CatalogueSourceError {
                    source: NativeErrorSource::SkillSource {
                        actual_path: actual_path.clone(),
                    },
                    reason: CatalogueReadFailure::Unavailable,
                }),
            }
        } else {
            distinct.push(skill);
        }
    }
    let skills = distinct;
    if cancelled.is_cancelled() {
        return Err(CatalogueReadFailure::Cancelled);
    }
    Ok(match limitation {
        Some(reason) => incomplete_skills(skills, errors, reason),
        None => CatalogueSection::Supported {
            items: skills,
            enumeration: SourceEnumeration::Accounted,
            errors,
        },
    })
}

fn read_settings_if_present(path: &Path) -> Result<NativeSkillSettings, CatalogueReadFailure> {
    match fs::metadata(path) {
        Ok(_) => super::claude_plugins::read_json(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(NativeSkillSettings::default())
        }
        Err(_) => Err(CatalogueReadFailure::Unavailable),
    }
}

fn incomplete_skills(
    items: Vec<NativeSkill>,
    errors: Vec<CatalogueSourceError>,
    reason: CatalogueLimitation,
) -> CatalogueSection<NativeSkill> {
    CatalogueSection::Supported {
        items,
        errors,
        enumeration: SourceEnumeration::Incomplete {
            limitations: CatalogueLimitations::new(reason, Vec::new()),
        },
    }
}

fn map_claude_file_failure(
    reason: acp_client_runtime::native_capabilities::ClaudeFilesystemReadError,
) -> CatalogueReadFailure {
    use acp_client_runtime::native_capabilities::ClaudeFilesystemReadError as Failure;
    match reason {
        Failure::Cancelled => CatalogueReadFailure::Cancelled,
        Failure::Unreadable => CatalogueReadFailure::Unavailable,
        Failure::InvalidScope => CatalogueReadFailure::InvalidScope,
        Failure::AmbiguousOrigin => CatalogueReadFailure::AmbiguousSelector,
        Failure::UnsupportedVersion | Failure::UnqualifiedSource | Failure::InvalidMetadata => {
            CatalogueReadFailure::InvalidNativeData
        }
    }
}

fn project_claude_file_skill(skill: ClaudeFileSkill) -> Result<NativeSkill, CatalogueReadFailure> {
    let lowering =
        qualified_claude_command(&skill.canonical_name, skill.native_display_name.as_deref());
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
        native_scope: match skill.scope {
            ClaudeFileSkillScope::User => NativeScope::User,
            ClaudeFileSkillScope::Workspace => NativeScope::Workspace,
            ClaudeFileSkillScope::Managed => NativeScope::Managed,
        },
        availability: if skill.user_invocable {
            NativeAvailability::Available
        } else {
            NativeAvailability::Disabled
        },
        display_version: skill.display_version.map(CapabilityName::new).transpose()?,
        plugin_id: None,
        lowering,
    })
}

/// Qualify only canonical ASCII spellings. The Host also requires the exact
/// native advertisement and rejects file/name collisions in the complete view.
pub(super) fn qualified_claude_command(name: &str, display: Option<&str>) -> NativeSkillLowering {
    if display.is_some_and(|display| display != name)
        || name.to_ascii_lowercase().contains("anthropic-skills")
        || name.len() > 256
    {
        return NativeSkillLowering::Unqualified {
            reason: LoweringUnsupported::AmbiguousNativeName,
        };
    }
    match NativeCommandName::new(name.to_owned()) {
        Ok(canonical_command) => NativeSkillLowering::Qualified { canonical_command },
        Err(_) => NativeSkillLowering::Unqualified {
            reason: LoweringUnsupported::UnsupportedInputForm,
        },
    }
}

#[cfg(test)]
#[path = "claude_skills_tests.rs"]
mod tests;
