use collaboration_central_core::SignInTrustSettings;
use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum ServeConfigurationError {
    #[error("could not read the central serve configuration")]
    Read(#[from] std::io::Error),
    #[error("invalid central serve configuration")]
    Decode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ServeConfiguration {
    pub listen: SocketAddr,
    pub root: PathBuf,
    pub sign_in: SignInTrustSettings,
}

impl ServeConfiguration {
    pub(super) fn read(path: &Path) -> Result<Self, ServeConfigurationError> {
        let input = std::fs::read_to_string(path)?;
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
        jsonc_parser::parse_to_serde_value(&input, &options)
            .map_err(|_| ServeConfigurationError::Decode)
    }
}

#[cfg(test)]
#[path = "serve_configuration_test.rs"]
mod tests;
