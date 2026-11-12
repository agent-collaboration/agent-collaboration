//! Owner-edited Router settings, read from `settings.jsonc` at each Host start.
//!
//! The file holds two independent sections, `providers` and `ports`. Each
//! section keeps its own error: a broken `providers` section leaves Claude Code
//! and Cursor unavailable, and a broken `ports` section stops the Host. A file
//! that is not valid JSONC, or that has a duplicate or unknown member at the top
//! level, fails both sections. A section that is absent takes its defaults; an
//! absent file gets a `providers` section written with today's defaults so the
//! owner has a file to edit. A file that exists is never rewritten.

use crate::provider_settings_section::ProviderConfigurationEntries;
use serde::de::DeserializeOwned;
use std::{
    collections::BTreeSet,
    fmt, fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Name of the settings file at the Router root.
pub const ROUTER_SETTINGS_FILE_NAME: &str = "settings.jsonc";

/// Returns the settings file path for one Router root.
#[must_use]
pub fn router_settings_path(router_root: &Path) -> PathBuf {
    router_root.join(ROUTER_SETTINGS_FILE_NAME)
}

/// One independently checked section of `settings.jsonc`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouterSettingsSection {
    Providers,
    Ports,
}

impl RouterSettingsSection {
    const fn member_name(self) -> &'static str {
        match self {
            Self::Providers => "providers",
            Self::Ports => "ports",
        }
    }
}

impl fmt::Display for RouterSettingsSection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "\"{}\"", self.member_name())
    }
}

#[derive(Debug, Error)]
pub enum RouterSettingsError {
    #[error("cannot read or create Router settings {path}: {source}")]
    FileIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("malformed Router settings {path}: {message}")]
    MalformedFile { path: PathBuf, message: String },
    #[error("invalid {section} section in Router settings {path}: {message}")]
    InvalidSection {
        path: PathBuf,
        section: RouterSettingsSection,
        message: String,
    },
}

/// A section read from the settings file.
pub(crate) struct RouterSettingsSectionRead<T> {
    pub(crate) path: PathBuf,
    /// Whether `settings.jsonc` existed when it was read.
    pub(crate) file_exists: bool,
    /// The parsed section, or `None` when the file or the section is absent.
    pub(crate) value: Option<T>,
}

impl<T> RouterSettingsSectionRead<T> {}

/// Reads one section without writing anything; an absent file or section is `None`.
pub(crate) fn read_router_settings_section<T: DeserializeOwned>(
    router_root: &Path,
    section: RouterSettingsSection,
) -> Result<RouterSettingsSectionRead<T>, RouterSettingsError> {
    let path = router_settings_path(router_root);
    let bytes = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(RouterSettingsError::FileIo { path, source }),
    };
    let file_exists = bytes.is_some();
    let value = match bytes {
        None => None,
        Some(bytes) => parse_section(&path, &bytes, section)?,
    };
    Ok(RouterSettingsSectionRead {
        path,
        file_exists,
        value,
    })
}

/// One section as JSON, read and checked the way the Host reads it; `None` when
/// the file or the section is absent.
pub fn read_router_settings_section_json(
    router_root: &Path,
    section: RouterSettingsSection,
) -> Result<Option<serde_json::Value>, RouterSettingsError> {
    Ok(read_router_settings_section::<serde_json::Value>(router_root, section)?.value)
}

/// Loads the `providers` section, creating the settings file with provider
/// defaults when it does not exist yet.
pub fn load_or_create_provider_settings(
    router_root: &Path,
) -> Result<ProviderConfigurationEntries, RouterSettingsError> {
    let section = RouterSettingsSection::Providers;
    let read = read_router_settings_section::<ProviderConfigurationEntries>(router_root, section)?;
    if read.file_exists {
        return validated_providers(read);
    }
    create_default_settings(router_root, &read.path)?;
    validated_providers(read_router_settings_section(router_root, section)?)
}

fn validated_providers(
    read: RouterSettingsSectionRead<ProviderConfigurationEntries>,
) -> Result<ProviderConfigurationEntries, RouterSettingsError> {
    let RouterSettingsSectionRead { path, value, .. } = read;
    match value {
        None => Ok(ProviderConfigurationEntries::default()),
        Some(entries) => {
            entries
                .validated()
                .map_err(|message| RouterSettingsError::InvalidSection {
                    path,
                    section: RouterSettingsSection::Providers,
                    message,
                })
        }
    }
}

fn create_default_settings(router_root: &Path, path: &Path) -> Result<(), RouterSettingsError> {
    use std::io::Write as _;
    let file_io = |source| RouterSettingsError::FileIo {
        path: path.to_owned(),
        source,
    };
    fs::create_dir_all(router_root).map_err(file_io)?;
    let providers = serde_json::to_string_pretty(&ProviderConfigurationEntries::default())
        .map_err(|error| RouterSettingsError::MalformedFile {
            path: path.to_owned(),
            message: error.to_string(),
        })?
        .replace('\n', "\n  ");
    let contents = format!(
        "// Router settings, edited by hand and read at each Host start.\n\
         // Sections: \"providers\" and \"ports\".\n\
         {{\n  \"providers\": {providers}\n}}\n"
    );
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file
            .write_all(contents.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(file_io),
        // A concurrent start created it first; the caller reads that file.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(file_io(source)),
    }
}

fn parse_section<T: DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
    section: RouterSettingsSection,
) -> Result<Option<T>, RouterSettingsError> {
    let malformed = |message: String| RouterSettingsError::MalformedFile {
        path: path.to_owned(),
        message,
    };
    let text = std::str::from_utf8(bytes).map_err(|error| malformed(error.to_string()))?;
    let options = jsonc_parser::ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    };
    let parsed = jsonc_parser::parse_to_ast(text, &Default::default(), &options)
        .map_err(|error| malformed(error.to_string()))?;
    let value = parsed
        .value
        .ok_or_else(|| malformed("the file holds no JSON object".to_owned()))?;
    let jsonc_parser::ast::Value::Object(object) = value else {
        return Err(malformed("the top level must be an object".to_owned()));
    };
    // Only top-level faults belong to the whole file; anything inside a section,
    // including a repeated member, is that section's own error.
    let mut names = BTreeSet::new();
    for property in &object.properties {
        let name = property.name.as_str();
        if !names.insert(name) {
            return Err(malformed(format!(
                "member \"{name}\" appears more than once"
            )));
        }
        if !["providers", "ports"].contains(&name) {
            return Err(malformed(format!(
                "unknown member \"{name}\" (expected \"providers\" or \"ports\")"
            )));
        }
    }
    let invalid_section = |message: String| RouterSettingsError::InvalidSection {
        path: path.to_owned(),
        section,
        message,
    };
    let Some(property) = object
        .properties
        .into_iter()
        .find(|property| property.name.as_str() == section.member_name())
    else {
        return Ok(None);
    };
    reject_duplicate_members(&property.value)
        .map_err(|member| invalid_section(format!("member \"{member}\" appears more than once")))?;
    serde_json::from_value(serde_json::Value::from(property.value))
        .map(Some)
        .map_err(|error| invalid_section(error.to_string()))
}

fn reject_duplicate_members<'a>(value: &'a jsonc_parser::ast::Value<'a>) -> Result<(), String> {
    match value {
        jsonc_parser::ast::Value::Object(object) => {
            let mut names = BTreeSet::new();
            for property in &object.properties {
                if !names.insert(property.name.as_str()) {
                    return Err(property.name.as_str().to_owned());
                }
                reject_duplicate_members(&property.value)?;
            }
        }
        jsonc_parser::ast::Value::Array(array) => {
            for element in &array.elements {
                reject_duplicate_members(element)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "router_settings_test.rs"]
mod tests;
