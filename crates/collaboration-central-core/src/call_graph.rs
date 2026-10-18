//! The only synchronous object edges. Rank checks and wire checks share this table.
use crate::*;
use collaboration_replication_protocol::{InstallationId, ProjectId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallerKind {
    Edge,
    Session,
    Registry,
    Copy,
    Fleet,
    Delivery,
}

mod sealed {
    pub trait Kind {}
    pub trait Edge<To> {}
}

pub trait ObjectKind: sealed::Kind + Clone + std::fmt::Debug + PartialEq {
    const RANK: u8;
    const KIND: CallerKind;
    type Address: Clone + std::fmt::Debug + PartialEq + Serialize + serde::de::DeserializeOwned;
    type Call: Clone + std::fmt::Debug + PartialEq + Serialize + serde::de::DeserializeOwned;
    type Result: Clone + std::fmt::Debug + PartialEq + Serialize + serde::de::DeserializeOwned;
    type Effect: Clone;
    fn address(target: &Self::Address) -> ObjectAddress;
}

/// Compile-time permission to issue a synchronous call. A lower-ranked target
/// cannot be added without the edge table's const assertion failing.
///
/// ```
/// use collaboration_central_core::{MayCall, ObjectKind, SessionKind, RegistryKind};
/// fn permitted<From: MayCall<To>, To: ObjectKind>() {}
/// permitted::<SessionKind, RegistryKind>();
/// ```
///
/// ```compile_fail
/// use collaboration_central_core::{MayCall, ObjectKind, CopyKind, RegistryKind};
/// fn permitted<From: MayCall<To>, To: ObjectKind>() {}
/// permitted::<CopyKind, RegistryKind>();
/// ```
pub trait MayCall<To: ObjectKind>: ObjectKind + sealed::Edge<To> {}

macro_rules! object_kind {
    ($name:ident,$rank:literal,$kind:ident,$address:ty,$call:ty,$result:ty,$effect:ty,$physical:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
        pub struct $name;
        impl sealed::Kind for $name {}
        impl ObjectKind for $name {
            const RANK: u8 = $rank;
            const KIND: CallerKind = CallerKind::$kind;
            type Address = $address;
            type Call = $call;
            type Result = $result;
            type Effect = $effect;
            fn address(target: &Self::Address) -> ObjectAddress {
                ($physical)(target)
            }
        }
    };
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EdgeCall {}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EdgeResult {}
object_kind!(EdgeKind, 0, Edge, (), EdgeCall, EdgeResult, (), |_: &()| {
    ObjectAddress::Registry
});
object_kind!(
    SessionKind,
    1,
    Session,
    InstallationId,
    SessionCall,
    SessionResult,
    SessionEffect,
    |v: &InstallationId| ObjectAddress::Connection(v.clone())
);
object_kind!(
    RegistryKind,
    2,
    Registry,
    (),
    RegistryCall,
    RegistryResult,
    RegistryEffect,
    |_: &()| ObjectAddress::Registry
);
object_kind!(
    CopyKind,
    3,
    Copy,
    ProjectId,
    CopyCall,
    CopyResult,
    CopyEffect,
    |v: &ProjectId| ObjectAddress::Copy(v.clone())
);
object_kind!(
    FleetKind,
    4,
    Fleet,
    (),
    FleetCall,
    FleetResult,
    FleetEffect,
    |_: &()| ObjectAddress::Fleet
);
object_kind!(
    DeliveryKind,
    5,
    Delivery,
    InstallationId,
    DeliveryCall,
    DeliveryResult,
    DeliveryEffect,
    |v: &InstallationId| ObjectAddress::Connection(v.clone())
);

macro_rules! allowed_edges {
    ($( $from:ident => $to:ident ),+ $(,)?) => {
        $(const _: ()=assert!(<$to as ObjectKind>::RANK > <$from as ObjectKind>::RANK);
          impl sealed::Edge<$to> for $from {}
          impl MayCall<$to> for $from {})+
        impl CallerKind {
            #[must_use]
            pub const fn may_call(self, target: Self) -> bool {
                matches!((self,target), $( ($from::KIND,$to::KIND) )|+)
            }
        }
    }
}
allowed_edges!(
    EdgeKind=>SessionKind, EdgeKind=>RegistryKind, EdgeKind=>CopyKind, EdgeKind=>FleetKind,
    SessionKind=>RegistryKind, SessionKind=>CopyKind, SessionKind=>FleetKind, SessionKind=>DeliveryKind,
    RegistryKind=>CopyKind, RegistryKind=>FleetKind, RegistryKind=>DeliveryKind,
    CopyKind=>FleetKind, FleetKind=>DeliveryKind,
);
