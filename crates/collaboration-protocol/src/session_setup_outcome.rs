//! A known harness conversation can be unusable without having been refused by its provider.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SessionSetupAbandonmentCause {
    ModelMismatch {},
    EffectiveWorkingDirectoryMismatch {},
    WorkspaceRootsMismatch {},
    InitialSettingRefused { setting: crate::ProviderSettingName },
    SetupInterrupted {},
}
