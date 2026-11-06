//! A caller's face and address are bound by the core that owns the object.

use crate::{
    AccessCaller, AccessClientId, CopyAddress, EdgeKind, FleetAddress, ObjectAddress, ObjectKind,
    OwnerIdentity, SessionAddress,
};
use fleet_identity_protocol::OrganizationId;
use std::marker::PhantomData;

pub struct CallerContext<Face: ObjectKind> {
    address: Face::Address,
    face: PhantomData<fn() -> Face>,
    pub(crate) authority: CallAuthority,
}

impl<Face: ObjectKind> CallerContext<Face> {
    pub(crate) fn for_object(address: Face::Address) -> Self {
        Self {
            address,
            face: PhantomData,
            authority: CallAuthority::Object,
        }
    }

    pub(crate) fn address(&self) -> ObjectAddress {
        Face::address(&self.address)
    }

    pub(crate) fn organization(&self) -> &OrganizationId {
        Face::organization(&self.address)
    }

    pub(crate) fn fleet_address(&self) -> FleetAddress {
        FleetAddress::new(self.organization().clone())
    }

    pub(crate) fn copy_address(&self, database: fleet_sync_protocol::DatabaseId) -> CopyAddress {
        CopyAddress {
            organization: self.organization().clone(),
            database,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum CallAuthority {
    Object,
    Machine {
        installation: message_board::ServiceId,
    },
    Owner(OwnerIdentity),
    AccessMachine(AccessClientId),
}
impl CallerContext<EdgeKind> {
    pub(crate) fn for_owner(owner: OwnerIdentity, organization: OrganizationId) -> Self {
        Self {
            address: FleetAddress::new(organization),
            face: PhantomData,
            authority: CallAuthority::Owner(owner),
        }
    }

    pub(crate) fn for_verified_edge(
        caller: AccessCaller,
        organization: OrganizationId,
    ) -> Result<Self, crate::AccessRefusal> {
        let authority = match caller {
            AccessCaller::Machine { client_id } => CallAuthority::AccessMachine(client_id),
            owner @ AccessCaller::Owner { .. } => {
                CallAuthority::Owner(OwnerIdentity::from_access(owner)?)
            }
        };
        Ok(Self {
            address: FleetAddress::new(organization),
            face: PhantomData,
            authority,
        })
    }
}

impl CallerContext<crate::SessionKind> {
    pub(crate) fn for_machine(
        organization: OrganizationId,
        installation: message_board::ServiceId,
    ) -> Self {
        Self {
            address: SessionAddress {
                organization,
                installation: installation.clone(),
            },
            face: PhantomData,
            authority: CallAuthority::Machine { installation },
        }
    }
}
