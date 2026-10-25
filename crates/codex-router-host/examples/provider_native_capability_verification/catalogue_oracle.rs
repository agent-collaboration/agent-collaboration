//! J1 oracles: skill and plugin discovery and management rejection.
//!
//! Catalogue authority is the bound provider: its advertised names, its own skill and
//! command roots, its plugin and marketplace exports and its plugin files. Expected
//! values never come from the candidate's response. An explicit JSON-RPC error is
//! `ExplicitUnsupported`, never a pass; a wrong build returns `Err`.
use crate::native_file_facts::{
    canonical, claude_source_candidates, is_skill_source, natively_disabled, same_path,
};
use crate::verification_models::{
    CapabilityStatus, FixtureRecord, NativeMarketplace, NativePluginFacts, NativePluginRow,
    PluginListView, PluginReadView, ProviderEndpoint, RpcReply, SkillsEntryView, SkillsListView,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// What the bound provider itself loads.
#[derive(Clone, Debug)]
pub(crate) struct NativeSkillAuthority {
    pub(crate) provider: ProviderEndpoint,
    pub(crate) advertised_names: BTreeSet<String>,
    pub(crate) home_skill_roots: Vec<PathBuf>,
    pub(crate) workspace_skill_dirs: Vec<String>,
    pub(crate) home_command_roots: Vec<PathBuf>,
    pub(crate) workspace_command_dirs: Vec<String>,
    pub(crate) project_root_marker: Option<String>,
    /// Active plugin folders and the namespace the provider advertises their skills under.
    pub(crate) plugin_namespaces: Vec<(PathBuf, String)>,
    /// The installed plugins' exact declared skill files (`plugin_skill_files`): for
    /// Claude these, not every file inside a plugin folder, are plugin skill sources.
    pub(crate) plugin_skill_sources: Vec<PathBuf>,
}

impl NativeSkillAuthority {
    /// The nearest ancestor holding the marker, else the cwd itself (Cursor's installed
    /// `utils/git.ts` root finder returns its input when no `.git` is found).
    pub(crate) fn project_root(&self, cwd: &Path) -> PathBuf {
        let Some(marker) = &self.project_root_marker else {
            return cwd.to_owned();
        };
        cwd.ancestors()
            .find(|candidate| candidate.join(marker).exists())
            .unwrap_or(cwd)
            .to_owned()
    }

    /// Home and workspace skill roots, without plugin folders.
    pub(crate) fn ordinary_skill_roots(&self, cwd: &Path) -> Vec<PathBuf> {
        let root = self.project_root(cwd);
        let mut roots = self.home_skill_roots.clone();
        roots.extend(self.workspace_skill_dirs.iter().map(|dir| root.join(dir)));
        roots
    }

    pub(crate) fn skill_roots(&self, cwd: &Path) -> Vec<PathBuf> {
        let mut roots = self.ordinary_skill_roots(cwd);
        roots.extend(
            self.plugin_namespaces
                .iter()
                .map(|(folder, _)| folder.clone()),
        );
        roots
    }

    pub(crate) fn command_roots(&self, cwd: &Path) -> Vec<PathBuf> {
        let root = self.project_root(cwd);
        let mut roots = self.home_command_roots.clone();
        roots.extend(self.workspace_command_dirs.iter().map(|dir| root.join(dir)));
        roots
    }

    fn namespace_for(&self, path: &Path) -> Option<&str> {
        let resolved = canonical(path);
        self.plugin_namespaces
            .iter()
            .find(|(folder, _)| resolved.starts_with(canonical(folder)))
            .map(|(_, namespace)| namespace.as_str())
    }

    /// Advertised under the namespace its file's owning plugin implies; any other skill
    /// only under its exact name. A same-named skill of another plugin never matches.
    pub(crate) fn natively_advertised(&self, name: &str, path: &Path) -> bool {
        match self.namespace_for(path) {
            None => self.advertised_names.contains(name),
            Some(namespace) if name.starts_with(&format!("{namespace}:")) => {
                self.advertised_names.contains(name)
            }
            Some(namespace) => self
                .advertised_names
                .contains(&format!("{namespace}:{name}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillCatalogueCheck {
    pub(crate) status: CapabilityStatus,
    pub(crate) skill_count: usize,
    pub(crate) error_count: usize,
    pub(crate) fixture_path: Option<String>,
}

fn unsupported_skills(message: &str) -> SkillCatalogueCheck {
    SkillCatalogueCheck {
        status: CapabilityStatus::ExplicitUnsupported {
            message: message.to_owned(),
        },
        skill_count: 0,
        error_count: 0,
        fixture_path: None,
    }
}

fn workspace_entry(reply: &RpcReply, cwd: &Path) -> Result<SkillsEntryView, String> {
    let view: SkillsListView = serde_json::from_value(reply.result.clone().unwrap_or_default())
        .map_err(|error| format!("skills/list result does not decode: {error}"))?;
    let mut entries = view
        .data
        .into_iter()
        .filter(|entry| same_path(Path::new(&entry.cwd), cwd));
    match (entries.next(), entries.next()) {
        (Some(entry), None) => Ok(entry),
        _ => Err("skills/list returned no single entry for the requested cwd".to_owned()),
    }
}

/// Every row is a real file under the provider's own roots, and its availability is
/// judged from native facts, never from the row's flag:
/// - an enabled row is advertised under its file's namespace and carries no native
///   disabled fact, so a substituted Codex catalogue fails;
/// - a disabled row (FilesystemCatalogues keeps real skipped files) carries a native
///   disabled fact in its own file; a row merely marked disabled fails;
/// - a disabled file never stands in for an advertised active definition of its name.
pub(crate) fn require_truthful_skill_entry(
    entry: &SkillsEntryView,
    cwd: &Path,
    authority: &NativeSkillAuthority,
) -> Result<(), String> {
    let skill_roots = authority.skill_roots(cwd);
    let command_roots = authority.command_roots(cwd);
    let mut roots = skill_roots.clone();
    roots.extend(command_roots.iter().cloned());
    // Claude: membership is the exact native source enumeration, judged on logical and
    // canonical identity. Cursor keeps its existing recursive containment.
    let claude_sources = (authority.provider == ProviderEndpoint::ClaudeLocal).then(|| {
        claude_source_candidates(
            &authority.ordinary_skill_roots(cwd),
            &command_roots,
            &authority.plugin_skill_sources,
        )
    });
    let is_member = |path: &Path| match &claude_sources {
        Some(sources) => sources.is_source(path),
        None => {
            let resolved = canonical(path);
            roots
                .iter()
                .any(|root| resolved.starts_with(canonical(root)))
        }
    };
    for skill in &entry.skills {
        let path = Path::new(&skill.path);
        if !path.is_absolute() || !path.exists() {
            return Err(format!(
                "skill {:?} has no real filesystem origin",
                skill.name
            ));
        }
        if !is_member(path) {
            return Err(format!(
                "skill {:?} lies outside the provider's own roots",
                skill.name
            ));
        }
        let disabled = natively_disabled(path, authority.provider);
        if skill.enabled && disabled {
            return Err(format!(
                "natively disabled skill {:?} reported enabled",
                skill.name
            ));
        }
        if skill.enabled && !authority.natively_advertised(&skill.name, path) {
            return Err(format!(
                "skill {:?} is not loaded by the bound provider",
                skill.name
            ));
        }
        if !skill.enabled && !disabled {
            return Err(format!(
                "skill {:?} reported disabled without a qualified native disabled fact",
                skill.name
            ));
        }
    }
    let enabled_names: BTreeSet<&str> = entry
        .skills
        .iter()
        .filter(|skill| skill.enabled)
        .map(|skill| skill.name.as_str())
        .collect();
    if let Some(shadow) = entry.skills.iter().find(|skill| {
        !skill.enabled
            && authority.advertised_names.contains(&skill.name)
            && !enabled_names.contains(skill.name.as_str())
    }) {
        return Err(format!(
            "disabled file shadows the active native definition of {:?}",
            shadow.name
        ));
    }
    let error_is_source = |path: &Path| match &claude_sources {
        Some(sources) => path.is_absolute() && sources.is_source(path),
        None => is_skill_source(path, &skill_roots, &command_roots),
    };
    if entry
        .errors
        .iter()
        .any(|error| !error_is_source(Path::new(&error.path)))
    {
        return Err("skill error names no actual native skill source".to_owned());
    }
    Ok(())
}

/// J1 skills: the controlled workspace skill appears with its real origin (C1, R1).
pub(crate) fn check_skill_catalogue(
    reply: &RpcReply,
    fixture: &FixtureRecord,
    authority: &NativeSkillAuthority,
) -> Result<SkillCatalogueCheck, String> {
    // The provider's own command list is the native reference that the fixture loaded.
    if !authority.advertised_names.contains(&fixture.skill_name) {
        return Err("native provider did not load the controlled fixture skill".to_owned());
    }
    if let Some(error) = &reply.error {
        return Ok(unsupported_skills(&error.message));
    }
    let entry = workspace_entry(reply, &fixture.workspace)?;
    require_truthful_skill_entry(&entry, &fixture.workspace, authority)?;
    let matches: Vec<_> = entry
        .skills
        .iter()
        .filter(|skill| skill.name == fixture.skill_name)
        .collect();
    let [skill] = matches.as_slice() else {
        return Err("controlled native skill missing: empty or foreign catalogue".to_owned());
    };
    let path = Path::new(&skill.path);
    let fixture_folder = fixture.skill_file.parent().unwrap_or(&fixture.skill_file);
    if !(same_path(path, &fixture.skill_file) || same_path(path, fixture_folder)) {
        return Err("controlled native skill reported with a different origin".to_owned());
    }
    if !skill.enabled {
        return Err("enabled native skill reported disabled".to_owned());
    }
    if skill.scope != "repo" {
        return Err("workspace skill reported outside the workspace scope".to_owned());
    }
    Ok(SkillCatalogueCheck {
        status: CapabilityStatus::Supported,
        skill_count: entry.skills.len(),
        error_count: entry.errors.len(),
        fixture_path: Some(skill.path.clone()),
    })
}

/// J1 negative: another workspace never borrows the fixture workspace's skills.
pub(crate) fn check_wrong_context_catalogue(
    reply: &RpcReply,
    fixture: &FixtureRecord,
    authority: &NativeSkillAuthority,
) -> Result<SkillCatalogueCheck, String> {
    if let Some(error) = &reply.error {
        return Ok(unsupported_skills(&error.message));
    }
    let entry = workspace_entry(reply, &fixture.empty_workspace)?;
    require_truthful_skill_entry(&entry, &fixture.empty_workspace, authority)?;
    let fixture_workspace = canonical(&fixture.workspace);
    let empty_workspace = canonical(&fixture.empty_workspace);
    for skill in &entry.skills {
        let path = canonical(Path::new(&skill.path));
        if skill.name == fixture.skill_name
            || skill.name == fixture.second_skill_name
            || path.starts_with(&fixture_workspace)
        {
            return Err("catalogue borrowed another workspace's native skill".to_owned());
        }
        if skill.scope == "repo" && !path.starts_with(&empty_workspace) {
            return Err("repo-scope skill outside the requested workspace".to_owned());
        }
    }
    Ok(SkillCatalogueCheck {
        status: CapabilityStatus::Supported,
        skill_count: entry.skills.len(),
        error_count: entry.errors.len(),
        fixture_path: None,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginCatalogueCheck {
    pub(crate) status: CapabilityStatus,
    pub(crate) installed_ids: Vec<String>,
    pub(crate) defaulted_policy_ids: Vec<String>,
    pub(crate) load_error_paths: Vec<String>,
    /// Native rows with special keys (`@npm`, `session[...]`): outside the filesystem
    /// marketplace view and not judged here; reported, never counted as listed.
    pub(crate) unqualified_native_ids: Vec<String>,
}

/// J1 plugins: the installed list is exactly the provider's own export (C1, C4, D2).
/// Marketplace rows and load errors name only the actual manifest file from the
/// provider's own marketplace export; package folders or name guesses never qualify.
pub(crate) fn check_plugin_catalogue(
    reply: &RpcReply,
    native_rows: &[NativePluginRow],
    marketplaces: &[NativeMarketplace],
) -> Result<PluginCatalogueCheck, String> {
    if let Some(error) = &reply.error {
        return Ok(PluginCatalogueCheck {
            status: CapabilityStatus::ExplicitUnsupported {
                message: error.message.clone(),
            },
            installed_ids: Vec::new(),
            defaulted_policy_ids: Vec::new(),
            load_error_paths: Vec::new(),
            unqualified_native_ids: Vec::new(),
        });
    }
    let view: PluginListView = serde_json::from_value(reply.result.clone().unwrap_or_default())
        .map_err(|error| format!("plugin/list result does not decode: {error}"))?;
    let manifests: BTreeMap<&str, &Path> = marketplaces
        .iter()
        .filter_map(|market| Some((market.name.as_str(), market.manifest_path.as_deref()?)))
        .collect();
    let native: BTreeMap<&str, &NativePluginRow> = native_rows
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    let mut listed = BTreeMap::new();
    for marketplace in &view.marketplaces {
        let manifest = manifests.get(marketplace.name.as_str()).ok_or_else(|| {
            format!(
                "marketplace {} has no manifest in the provider export",
                marketplace.name
            )
        })?;
        if !marketplace
            .path
            .as_deref()
            .is_some_and(|path| same_path(Path::new(path), manifest))
        {
            return Err(format!(
                "marketplace {} does not name its actual manifest",
                marketplace.name
            ));
        }
        for plugin in &marketplace.plugins {
            if plugin.remote_plugin_id.is_some() || plugin.share_context.is_some() {
                return Err(format!(
                    "plugin {} carries a manufactured remote identity",
                    plugin.id
                ));
            }
            if !plugin.installed {
                continue;
            }
            let row = native.get(plugin.id.as_str()).ok_or_else(|| {
                format!(
                    "plugin {} is outside the provider's installation",
                    plugin.id
                )
            })?;
            match row.regular_key() {
                Some((_, market)) if market == marketplace.name => {}
                Some(_) => {
                    return Err(format!(
                        "plugin {} listed under a guessed marketplace",
                        plugin.id
                    ));
                }
                None => {
                    return Err(format!(
                        "special native key {} listed as a marketplace package",
                        plugin.id
                    ));
                }
            }
            if plugin.enabled != row.enabled {
                return Err(format!(
                    "plugin {} enablement differs from the provider",
                    plugin.id
                ));
            }
            listed.insert(plugin.id.clone(), plugin);
        }
    }
    for error in &view.marketplace_load_errors {
        if !manifests
            .values()
            .any(|manifest| same_path(Path::new(&error.marketplace_path), manifest))
        {
            return Err("marketplace load error names no actual manifest file".to_owned());
        }
    }
    let mut unqualified_native_ids = Vec::new();
    for row in native_rows {
        if listed.contains_key(&row.id) {
            continue;
        }
        let Some((_, market)) = row.regular_key() else {
            unqualified_native_ids.push(row.id.clone());
            continue;
        };
        // Each omission needs its own marketplace's manifest error; another path excuses nothing.
        let explained = manifests.get(market).is_some_and(|manifest| {
            view.marketplace_load_errors
                .iter()
                .any(|error| same_path(Path::new(&error.marketplace_path), manifest))
        });
        if !explained {
            return Err(format!(
                "complete-looking list omits installed plugin {}",
                row.id
            ));
        }
    }
    Ok(PluginCatalogueCheck {
        status: CapabilityStatus::Supported,
        defaulted_policy_ids: listed
            .iter()
            .filter(|(_, plugin)| {
                plugin.install_policy == "AVAILABLE" && plugin.auth_policy == "ON_INSTALL"
            })
            .map(|(id, _)| id.clone())
            .collect(),
        installed_ids: listed.into_keys().collect(),
        load_error_paths: view
            .marketplace_load_errors
            .iter()
            .map(|error| error.marketplace_path.clone())
            .collect(),
        unqualified_native_ids,
    })
}

fn event_key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// J1 plugin/read: the exact selector resolves to the provider's own record (C1, C4).
/// Every required detail item comes from the plugin's own native facts (M1). Per-skill
/// enablement has native overrides this oracle has not qualified, so it is not asserted.
pub(crate) fn check_plugin_detail(
    reply: &RpcReply,
    native_row: &NativePluginRow,
    facts: &NativePluginFacts,
) -> Result<CapabilityStatus, String> {
    if let Some(error) = &reply.error {
        return Ok(CapabilityStatus::ExplicitUnsupported {
            message: error.message.clone(),
        });
    }
    let detail = serde_json::from_value::<PluginReadView>(reply.result.clone().unwrap_or_default())
        .map_err(|error| format!("plugin/read result does not decode: {error}"))?
        .plugin;
    if detail.summary.id != native_row.id {
        return Err("plugin/read resolved a different plugin".to_owned());
    }
    if detail.summary.enabled != native_row.enabled {
        return Err("plugin/read enablement differs from the provider".to_owned());
    }
    if native_row.regular_key().map(|(_, market)| market) != Some(detail.marketplace_name.as_str())
    {
        return Err("plugin/read reports a guessed marketplace".to_owned());
    }
    if detail.summary.remote_plugin_id.is_some() || detail.summary.share_context.is_some() {
        return Err("plugin/read carries a manufactured remote identity".to_owned());
    }
    if detail.summary.install_policy == "AVAILABLE" && detail.summary.auth_policy == "ON_INSTALL" {
        let text = detail
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        if !text.contains("codex") || !text.contains("default") {
            return Err("Codex policy defaults shown without attribution".to_owned());
        }
    }
    // Claude namespaces plugin skills as `plugin:skill`; either form names the same skill.
    let namespace = format!("{}:", detail.summary.name);
    let mut skills = BTreeMap::new();
    for skill in &detail.skills {
        let name = skill.name.strip_prefix(&namespace).unwrap_or(&skill.name);
        if skills.insert(name.to_owned(), skill).is_some() {
            return Err("plugin/read repeats a native skill".to_owned());
        }
    }
    if skills.keys().ne(facts.skill_descriptions.keys()) {
        return Err("plugin/read skills differ from the plugin's own skills".to_owned());
    }
    for (name, skill) in &skills {
        if facts.skill_descriptions.get(name) != Some(&skill.description) {
            return Err(format!("skill {name} description is not the native one"));
        }
    }
    let Some(hook_events) = &facts.hook_events else {
        return Err("plugin/read shows hooks the native files do not establish".to_owned());
    };
    let native_events: BTreeSet<String> =
        hook_events.iter().map(|event| event_key(event)).collect();
    let shown_events: BTreeSet<String> = detail
        .hooks
        .iter()
        .map(|hook| event_key(&hook.event_name))
        .collect();
    if shown_events != native_events {
        return Err("plugin/read hooks dropped or invented native hook events".to_owned());
    }
    let mut shown_servers = detail.mcp_servers.clone();
    shown_servers.sort();
    let mut native_servers = facts
        .mcp_servers
        .clone()
        .ok_or("native MCP declaration unobserved")?;
    native_servers.sort();
    if shown_servers != native_servers {
        return Err("plugin/read MCP servers differ from the native declaration".to_owned());
    }
    Ok(CapabilityStatus::Supported)
}

/// Installation management is rejected with no change to the native setup (C4).
pub(crate) fn check_management_rejected(
    reply: &RpcReply,
    native_before: &[NativePluginRow],
    native_after: &[NativePluginRow],
) -> Result<(), String> {
    if reply.error.is_none() {
        return Err("management write was accepted".to_owned());
    }
    if native_before != native_after {
        return Err("native installation changed around a rejected management write".to_owned());
    }
    Ok(())
}

/// J1 sample beside the fixture: a real installed plugin's own skill file appears by
/// path. Discovery only; nothing is executed.
pub(crate) fn check_installed_plugin_skill_sample(
    reply: &RpcReply,
    workspace: &Path,
    row: &NativePluginRow,
    skill_files: &[PathBuf],
) -> Result<String, String> {
    if reply.error.is_some() {
        return Err("skills/list failed, so no installed sample can be compared".to_owned());
    }
    let entry = workspace_entry(reply, workspace)?;
    entry
        .skills
        .iter()
        .find(|skill| {
            let path = Path::new(&skill.path);
            skill_files.iter().any(|file| {
                same_path(path, file) || file.parent().is_some_and(|folder| same_path(path, folder))
            })
        })
        .map(|skill| skill.path.clone())
        .ok_or_else(|| {
            format!(
                "installed plugin {} skills are missing from skills/list",
                row.id
            )
        })
}
