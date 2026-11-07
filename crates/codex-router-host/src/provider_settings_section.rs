//! The `providers` section of `settings.jsonc`: owner-edited provider defaults.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfigurationEntry {
    pub enabled: bool,
    pub executable: Option<PathBuf>,
    pub arguments: Vec<String>,
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
                },
                cursor: ProviderConfigurationEntry {
                    enabled: true,
                    executable: None,
                    arguments: vec!["acp".to_owned()],
                },
            },
        }
    }
}
