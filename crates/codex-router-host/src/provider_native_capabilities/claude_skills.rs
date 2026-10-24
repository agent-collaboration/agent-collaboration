//! Compose the native ordinary, legacy and installed-plugin filesystem views.
#[path = "claude_project_context.rs"]
mod project_context;
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
        plugins: Vec<NativePlugin>,
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
            read_native_claude_skill_section(&home, &config_home, &workspace, &plugins, &cancelled)
        })
        .await
        .map_err(|_| CatalogueReadFailure::Unavailable)?
    }
}

fn read_native_claude_skill_section(
    home: &Path,
    config_home: &Path,
    workspace: &Path,
    plugins: &[NativePlugin],
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
    let project_directories = project.skill_directories;
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
        roots.extend(project_directories.iter().map(|directory| ClaudeSkillRoot {
            directory: directory.join("skills"),
            scope: ClaudeFileSkillScope::Workspace,
        }));
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
        command_roots.extend(project_directories.iter().map(|directory| ClaudeSkillRoot {
            directory: directory.join("commands"),
            scope: ClaudeFileSkillScope::Workspace,
        }));
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
    for plugin in plugins {
        match &plugin.components.skills {
            NativeComponentSection::Observed {
                items,
                coverage: CatalogueCoverage::Complete,
            } => {
                for skill in items {
                    let mut skill = skill.clone();
                    if overrides.get(skill.name.as_str()) == Some(&ClaudeSkillOverride::Off) {
                        skill.availability = NativeAvailability::Disabled;
                    }
                    skills.push(skill);
                }
            }
            _ => limitation = Some(CatalogueLimitation::PartialNativeExport),
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
        lowering: NativeSkillLowering::Unqualified {
            reason: LoweringUnsupported::ScopeUnqualified,
        },
    })
}

#[cfg(test)]
#[path = "claude_skills_tests.rs"]
mod tests;
