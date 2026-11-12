//! Shared tool inputs for the catalog's argument-free tools.
use super::{Deserialize, schemars};

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyToolInput {}
