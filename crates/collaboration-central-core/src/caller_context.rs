//! A caller's face and address are bound by the core that owns the object.

use crate::{AccessCaller, AccessClientId, EdgeKind, ObjectAddress, ObjectKind, OwnerIdentity};
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
    Owner(OwnerIdentity),
    AccessMachine(AccessClientId),
}
impl CallerContext<EdgeKind> {
    pub(crate) fn for_owner(owner: OwnerIdentity) -> Self {
        Self {
            address: (),
            face: PhantomData,
            authority: CallAuthority::Owner(owner),
        }
    }

    pub(crate) fn for_verified_edge(caller: AccessCaller) -> Result<Self, crate::AccessRefusal> {
        let authority = match caller {
            AccessCaller::Machine { client_id } => CallAuthority::AccessMachine(client_id),
            owner @ AccessCaller::Owner { .. } => {
                CallAuthority::Owner(OwnerIdentity::from_access(owner)?)
            }
        };
        Ok(Self {
            address: (),
            face: PhantomData,
            authority,
        })
    }
}
