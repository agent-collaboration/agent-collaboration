//! The Router translates its policy at the runtime-free Fleet wire boundary.
use crate::{ProviderRequestedPolicy, RouterAccess};
use fleet_identity_protocol::{RemoteLaunchAccess, RemoteLaunchPolicy};

impl From<ProviderRequestedPolicy> for RemoteLaunchPolicy {
    fn from(policy: ProviderRequestedPolicy) -> Self {
        Self {
            access: match policy.access {
                RouterAccess::WriteRestricted => RemoteLaunchAccess::WriteRestricted,
                RouterAccess::WorkspaceWrite => RemoteLaunchAccess::WorkspaceWrite,
            },
        }
    }
}

impl From<RemoteLaunchPolicy> for ProviderRequestedPolicy {
    fn from(policy: RemoteLaunchPolicy) -> Self {
        Self {
            access: match policy.access {
                RemoteLaunchAccess::WriteRestricted => RouterAccess::WriteRestricted,
                RemoteLaunchAccess::WorkspaceWrite => RouterAccess::WorkspaceWrite,
            },
        }
    }
}
