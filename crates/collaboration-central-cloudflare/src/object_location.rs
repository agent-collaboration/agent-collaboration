//! The Durable Objects central addresses, and each one's namespace and name.
use serde::{Deserialize, Serialize};

/// The `x-central-object-address` header value: which object a call is for.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub(crate) enum ObjectAddress {
    Central(fleet_identity_protocol::OrganizationId),
    /// One workspace's store: it holds the tables and runs the statements it is given.
    WorkspaceStore(workspace_ids::WorkspaceId),
}

pub(crate) fn location(address: &ObjectAddress) -> (&'static str, String) {
    match address {
        ObjectAddress::Central(organization) => ("CENTRAL", organization.as_str().to_owned()),
        ObjectAddress::WorkspaceStore(workspace) => ("WORKSPACE_STORE", workspace.as_str().into()),
    }
}
