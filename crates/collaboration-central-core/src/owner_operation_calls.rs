//! Owner-only operations reuse the ordinary typed object-call envelope.
use durable_steps::CallTarget;
use fleet_sync_protocol::DatabaseId;
use message_board::ProjectId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub(crate) struct CopyPageLimit(u16);
impl TryFrom<u16> for CopyPageLimit {
    type Error = &'static str;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if !(1..=100).contains(&value) {
            return Err("copy page limit must be between1 and100");
        }
        Ok(Self(value))
    }
}
impl From<CopyPageLimit> for u16 {
    fn from(value: CopyPageLimit) -> u16 {
        value.0
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyPageRequest {
    pub after: Option<ProjectId>,
    pub limit: CopyPageLimit,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct ReadCopyDirectoryPage;
impl CallTarget for ReadCopyDirectoryPage {
    const KIND: &'static str = "central.owner-copy-page";
    type Request = CopyPageRequest;
    type Answer = CopyDirectoryPage;
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyOperations {
    pub database: DatabaseId,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum CopyOpsCall {
    Health,
    Repair,
    Migrate,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum CopyOpsAnswer {
    Health(Box<collaboration_central_admin_protocol::CopyHealth>),
    Repaired,
    Retired,
    Migration(CopyMigrationResult),
}
impl CallTarget for CopyOperations {
    const KIND: &'static str = "central.copy-operations";
    type Request = CopyOpsCall;
    type Answer = CopyOpsAnswer;
}

pub(crate) use collaboration_central_admin_protocol::{CopyDirectoryPage, CopyMigrationResult};
