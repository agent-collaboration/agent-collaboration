//! The only synchronous object edges. Rank checks and wire checks share this table.
use crate::{FleetCall, FleetResult, ObjectAddress, RegistryCall, RegistryResult};
use fleet_sync_protocol::DatabaseId;
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallerKind {
    Edge,
    Session,
    Registry,
    Copy,
    Fleet,
    Mailbox,
    Delivery,
}

mod sealed {
    pub trait Kind {}
    pub trait Edge<To> {}
}

pub trait ObjectKind:
    sealed::Kind + Clone + std::fmt::Debug + PartialEq + Send + Sync + 'static
{
    const RANK: u8;
    const KIND: CallerKind;
    type Address: Clone
        + std::fmt::Debug
        + PartialEq
        + Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + 'static;
    type Call: Clone
        + std::fmt::Debug
        + PartialEq
        + Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + 'static;
    type Result: Clone
        + std::fmt::Debug
        + PartialEq
        + Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + 'static;
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
pub trait MayCall<To: ObjectKind>: ObjectKind + sealed::Edge<To> {}

macro_rules! object_kind {
    ($name:ident,$rank:literal,$kind:ident,$address:ty,$call:ty,$result:ty,$physical:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
        pub struct $name;
        impl sealed::Kind for $name {}
        impl ObjectKind for $name {
            const RANK: u8 = $rank;
            const KIND: CallerKind = CallerKind::$kind;
            type Address = $address;
            type Call = $call;
            type Result = $result;
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
// This face accepts shared CallTargets only; it has no legacy WireCall family.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MailboxCall {}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MailboxResult {}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SharedCallOnly {}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SharedAnswerOnly {}
object_kind!(EdgeKind, 0, Edge, (), EdgeCall, EdgeResult, |_: &()| {
    ObjectAddress::Registry
});
object_kind!(
    SessionKind,
    1,
    Session,
    ServiceId,
    SharedCallOnly,
    SharedAnswerOnly,
    |v: &ServiceId| ObjectAddress::Connection(v.clone())
);
object_kind!(
    RegistryKind,
    2,
    Registry,
    (),
    RegistryCall,
    RegistryResult,
    |_: &()| ObjectAddress::Registry
);
object_kind!(
    CopyKind,
    3,
    Copy,
    DatabaseId,
    SharedCallOnly,
    SharedAnswerOnly,
    |v: &DatabaseId| ObjectAddress::Copy(*v)
);
object_kind!(FleetKind, 4, Fleet, (), FleetCall, FleetResult, |_: &()| {
    ObjectAddress::Fleet
});
object_kind!(
    DeliveryKind,
    5,
    Delivery,
    ServiceId,
    SharedCallOnly,
    SharedAnswerOnly,
    |v: &ServiceId| ObjectAddress::Connection(v.clone())
);
object_kind!(
    MailboxKind,
    4,
    Mailbox,
    ServiceId,
    MailboxCall,
    MailboxResult,
    |v: &ServiceId| ObjectAddress::Mailbox(v.clone())
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
    SessionKind=>MailboxKind, RegistryKind=>MailboxKind, MailboxKind=>DeliveryKind,
    CopyKind=>FleetKind, FleetKind=>DeliveryKind,
);
