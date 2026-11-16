//! The `providers` section of `settings.jsonc`: owner-edited provider defaults.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        }
    }
}

#[cfg(test)]
#[path = "provider_settings_section_test.rs"]
mod tests;
