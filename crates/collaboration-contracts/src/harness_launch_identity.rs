//! One canonical identity for the declared input; secret-derived fingerprints stay in memory.
use collaboration_protocol::{
    EndpointRef, NonEmptyText, ProviderIdentity, RouterAccess, SessionRef, UuidIdentity,
};
use fleet_identity_protocol::{AbsolutePath, ModelName};
use message_board::ProjectId;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Eq, PartialEq)]
pub struct HarnessMcpConfigurationIdentity([u8; 32]);
impl HarnessMcpConfigurationIdentity {
    #[must_use]
    pub fn from_sha256(digest: [u8; 32]) -> Self {
        Self(digest)
    }
    #[must_use]
    pub fn empty() -> Self {
        Self(Sha256::digest(b"{}").into())
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct HarnessLaunchIdentity([u8; 32]);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessForkInput {
    pub source: SessionRef,
    pub through_turn_id: Option<NonEmptyText>,
}

#[derive(Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HarnessNativePermissions {
    InheritBackend,
    Explicit {
        access: RouterAccess,
        created_by: ProviderIdentity,
        approver: ProviderIdentity,
    },
}

#[derive(Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HarnessDeclaredSettings {
    Native {
        model: Option<ModelName>,
        effort: Option<NonEmptyText>,
        permissions: HarnessNativePermissions,
        additional_directories: Vec<AbsolutePath>,
        fork: Option<HarnessForkInput>,
        root_message_id: Option<UuidIdentity>,
    },
    Provider {
        model: Option<NonEmptyText>,
        effort: Option<NonEmptyText>,
        access: RouterAccess,
        mode: Option<NonEmptyText>,
        created_by: ProviderIdentity,
        approver: ProviderIdentity,
    },
}

/// Deliberately has no Debug or Serialize: the identity is not a persisted request record.
#[derive(Clone)]
pub struct HarnessDeclaredLaunchInput {
    pub project_id: Option<ProjectId>,
    pub endpoint: EndpointRef,
    pub working_directory: AbsolutePath,
    pub settings: HarnessDeclaredSettings,
    pub user_mcp: HarnessMcpConfigurationIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("declared harness launch input could not be encoded")]
pub struct HarnessLaunchIdentityEncoding;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalLaunchInput<'input> {
    project_id: &'input Option<ProjectId>,
    endpoint: &'input EndpointRef,
    working_directory: &'input AbsolutePath,
    settings: &'input HarnessDeclaredSettings,
    user_mcp_fingerprint: &'input [u8; 32],
}

impl HarnessDeclaredLaunchInput {
    pub fn identity(&self) -> Result<HarnessLaunchIdentity, HarnessLaunchIdentityEncoding> {
        let canonical = CanonicalLaunchInput {
            project_id: &self.project_id,
            endpoint: &self.endpoint,
            working_directory: &self.working_directory,
            settings: &self.settings,
            user_mcp_fingerprint: &self.user_mcp.0,
        };
        let bytes = serde_json::to_vec(&canonical).map_err(|_| HarnessLaunchIdentityEncoding)?;
        Ok(HarnessLaunchIdentity(Sha256::digest(bytes).into()))
    }
}
