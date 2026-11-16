//! The `providers` section of `settings.jsonc`: owner-edited provider defaults.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The owner's default work roots (D1 R18): every worktree, the two log
/// repositories, the toolchain caches, system temp and all of /tmp/agents/.
const DEFAULT_WORK_ROOTS: &[&str] = &[
    "~/code/projects",
    "~/dev/memory-logs",
    "~/dev/session-logs",
    "~/.cargo",
    "~/.rustup",
    "~/Library/Caches",
    "~/.cache",
    "~/Library/pnpm",
    SYSTEM_TEMP,
    "/tmp/agents",
];
/// How the settings file names the system temp folder.
const SYSTEM_TEMP: &str = "$TMPDIR";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfigurationEntry {
    pub enabled: bool,
    pub executable: Option<PathBuf>,
    pub arguments: Vec<String>,
    /// Claude only: keep its prompt cache warm while a session is idle (D1 R16).
    /// Absent means the Claude default; the Cursor entry must not carry it.
    #[serde(default, rename = "cacheWarm", skip_serializing_if = "Option::is_none")]
    pub cache_warm: Option<CacheWarmSetting>,
}

/// How the Host keeps Claude's prompt cache warm: one short prompt every
/// `every_minutes` to an idle session, for `for_hours` after its last real turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheWarmSetting {
    pub enabled: bool,
    pub every_minutes: u32,
    pub for_hours: u32,
}

impl Default for CacheWarmSetting {
    fn default() -> Self {
        Self {
            enabled: true,
            every_minutes: 55,
            for_hours: 8,
        }
    }
}

impl ProviderConfigurationEntry {
    /// The setting this Claude entry runs with: what it says, or the default.
    #[must_use]
    pub fn effective_cache_warm(&self) -> CacheWarmSetting {
        self.cache_warm.unwrap_or_default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfigurationEntries {
    version: u8,
    providers: ProviderEntries,
    /// Where a session the Router starts in auto works without asking, on
    /// top of the agent's own global settings (D1 R18). `~/` names a folder
    /// under the owner's home and `$TMPDIR` the system temp folder; any other
    /// entry is an absolute path. A file written before this list existed
    /// gets the defaults.
    #[serde(default = "default_work_roots", rename = "workRoots")]
    work_roots: Vec<String>,
}

fn default_work_roots() -> Vec<String> {
    DEFAULT_WORK_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderEntries {
    claude: ProviderConfigurationEntry,
    cursor: ProviderConfigurationEntry,
}

impl ProviderConfigurationEntries {
    #[must_use]
    pub fn claude(&self) -> &ProviderConfigurationEntry {
        &self.providers.claude
    }

    #[must_use]
    pub fn cursor(&self) -> &ProviderConfigurationEntry {
        &self.providers.cursor
    }

    /// The work roots as absolute paths, with `~/` under `home` and
    /// `$TMPDIR` as `system_temp`.
    #[must_use]
    pub fn work_roots(&self, home: &Path, system_temp: &Path) -> Vec<PathBuf> {
        self.work_roots
            .iter()
            .filter_map(|root| expand_work_root(root, home, system_temp))
            .collect()
    }

    /// Accepts only the section version this Router understands.
    pub(crate) fn validated(self) -> Result<Self, String> {
        if self.version != 1 {
            return Err(format!("unsupported version {} (expected 1)", self.version));
        }
        if self.providers.cursor.cache_warm.is_some() {
            return Err(
                "\"cacheWarm\" is not allowed on the cursor entry: only Claude keeps a prompt cache warm"
                    .to_owned(),
            );
        }
        if let Some(root) = self
            .work_roots
            .iter()
            .find(|root| expand_work_root(root, Path::new("/"), Path::new("/")).is_none())
        {
            return Err(format!(
                "\"workRoots\" entry {root:?} must be an absolute path, start with \"~/\", or start with \"$TMPDIR\""
            ));
        }
        if let Some(setting) = &self.providers.claude.cache_warm {
            if setting.every_minutes == 0 {
                return Err("claude \"cacheWarm\" \"everyMinutes\" must be at least 1".to_owned());
            }
            if setting.for_hours == 0 {
                return Err("claude \"cacheWarm\" \"forHours\" must be at least 1".to_owned());
            }
        }
        Ok(self)
    }
}

impl Default for ProviderConfigurationEntries {
    fn default() -> Self {
        Self {
            version: 1,
            providers: ProviderEntries {
                claude: ProviderConfigurationEntry {
                    enabled: true,
                    executable: None,
                    arguments: Vec::new(),
                    cache_warm: None,
                },
                cursor: ProviderConfigurationEntry {
                    enabled: true,
                    executable: None,
                    arguments: vec!["acp".to_owned()],
                    cache_warm: None,
                },
            },
            work_roots: default_work_roots(),
        }
    }
}

/// One work-root entry as an absolute path; `None` when it is neither
/// absolute nor under `~/` or `$TMPDIR`.
fn expand_work_root(root: &str, home: &Path, system_temp: &Path) -> Option<PathBuf> {
    if let Some(rest) = root.strip_prefix("~/") {
        return Some(home.join(rest));
    }
    if root == SYSTEM_TEMP {
        return Some(system_temp.to_path_buf());
    }
    if let Some(rest) = root.strip_prefix("$TMPDIR/") {
        return Some(system_temp.join(rest));
    }
    Path::new(root).is_absolute().then(|| PathBuf::from(root))
}

#[cfg(test)]
#[path = "provider_settings_section_test.rs"]
mod tests;
