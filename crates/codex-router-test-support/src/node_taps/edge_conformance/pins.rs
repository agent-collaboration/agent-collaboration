//! Reading the pinned definitions in `verification/schemas/` and checking each against the
//! SHA-256 `pins.json` records for it (R50).

use super::super::json_path::at;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// A pinned file is missing, unreadable, or not the file its pin names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PinError {
    Missing {
        file: String,
        error: String,
    },
    HashChanged {
        file: String,
        pinned: String,
        found: String,
    },
    Invalid {
        file: String,
        error: String,
    },
}

impl std::fmt::Display for PinError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { file, error } => {
                write!(formatter, "pinned file {file} is missing: {error}")
            }
            Self::HashChanged {
                file,
                pinned,
                found,
            } => write!(
                formatter,
                "pinned file {file} has sha256 {found}, but pins.json pins {pinned}"
            ),
            Self::Invalid { file, error } => {
                write!(formatter, "pinned file {file} is invalid: {error}")
            }
        }
    }
}

/// The directory holding the pinned definitions and `pins.json`.
#[must_use]
pub fn pinned_schema_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../verification/schemas")
}

pub(super) const CODEX_BUNDLE: &str = "codex-app-server-0.166.0.experimental.json";
pub(super) const CODEX_PAIRING: &str = "codex-app-server-0.166.0.pairing.json";
pub(super) const ACP_SCHEMA: &str = "acp-schema-1.9.1.json";
pub(super) const CLAUDE_EXTRAS: &str = "claude-agent-acp-0.81.0.extensions.json";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The pinned files `pins.json` in `directory` names, parsed, each with its hash checked.
pub(super) struct PinnedFiles(HashMap<String, Value>);

impl PinnedFiles {
    pub(super) fn read(directory: &Path) -> Result<Self, PinError> {
        let pins_text =
            std::fs::read(directory.join("pins.json")).map_err(|error| PinError::Missing {
                file: "pins.json".to_owned(),
                error: error.to_string(),
            })?;
        let pins: Value =
            serde_json::from_slice(&pins_text).map_err(|error| PinError::Invalid {
                file: "pins.json".to_owned(),
                error: error.to_string(),
            })?;
        let mut files = HashMap::new();
        for pin in at(&pins, &["pins"]).as_array().into_iter().flatten() {
            let (Some(file), Some(pinned)) =
                (at(pin, &["file"]).as_str(), at(pin, &["sha256"]).as_str())
            else {
                return Err(PinError::Invalid {
                    file: "pins.json".to_owned(),
                    error: format!("a pin without file or sha256: {pin}"),
                });
            };
            let bytes = std::fs::read(directory.join(file)).map_err(|error| PinError::Missing {
                file: file.to_owned(),
                error: error.to_string(),
            })?;
            let found = sha256_hex(&bytes);
            if found != pinned {
                return Err(PinError::HashChanged {
                    file: file.to_owned(),
                    pinned: pinned.to_owned(),
                    found,
                });
            }
            let value = serde_json::from_slice(&bytes).map_err(|error| PinError::Invalid {
                file: file.to_owned(),
                error: error.to_string(),
            })?;
            files.insert(file.to_owned(), value);
        }
        Ok(Self(files))
    }

    pub(super) fn take(&mut self, file: &str) -> Result<Value, PinError> {
        self.0.remove(file).ok_or_else(|| PinError::Missing {
            file: file.to_owned(),
            error: "pins.json does not pin it".to_owned(),
        })
    }
}
