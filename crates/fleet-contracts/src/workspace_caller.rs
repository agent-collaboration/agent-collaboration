use fleet_identity_protocol::OrganizationId;
use workspace_model::Caller;

/// Transport-stamped context. No workspace allowance is accepted from the caller.
#[derive(Clone)]
pub struct WorkspaceCaller {
    pub organization_id: OrganizationId,
    pub caller: Caller,
    pub credential_hash: String,
}
