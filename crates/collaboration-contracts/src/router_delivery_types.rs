//! Router-local evidence changes no board delivery payload or wire identity.
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_protocol::{DeliveryReceipt, ProviderAcpRouteEvidence};
use fleet_sync_protocol::{ConsumerProtocol, DeliveryTypes};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};

/// Recipient-local evidence journaled with the SDK's outcome, outside its wire item.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouterLocalObservation {
    pub receipt: DeliveryReceipt,
    /// The scheduled retry time after a failed delivery.
    pub scheduled_retry_at: Option<SyncTime>,
    pub route_evidence: Option<ProviderAcpRouteEvidence>,
}

impl From<DeliveryReceipt> for RouterLocalObservation {
    fn from(receipt: DeliveryReceipt) -> Self {
        Self {
            receipt,
            scheduled_retry_at: None,
            route_evidence: None,
        }
    }
}

pub struct RouterDeliveryTypes;

impl DeliveryTypes for RouterDeliveryTypes {
    const MAX_SENDER_BYTES: u32 = BoardDeliveryTypes::MAX_SENDER_BYTES;
    const MAX_ADDRESS_BYTES: u32 = BoardDeliveryTypes::MAX_ADDRESS_BYTES;
    const MAX_ACCEPTANCE_BYTES: u32 = BoardDeliveryTypes::MAX_ACCEPTANCE_BYTES;
    const PER_ITEM_SETTLEMENT: bool = BoardDeliveryTypes::PER_ITEM_SETTLEMENT;
    fn max_acceptance_bytes_for(body: &Self::Body) -> u32 {
        BoardDeliveryTypes::max_acceptance_bytes_for(body)
    }
    const MAX_STATUS_ROUTE_BYTES: u32 = BoardDeliveryTypes::MAX_STATUS_ROUTE_BYTES;
    // This is a bug guard with generous headroom over the realistic multi-claim
    // rejection measured by the permanent test. The library alone checks it.
    // Keep the 1 MiB receipt allowance plus 32 KiB for the wrapper and captured
    // provider context, including maximal escaped session/binding identifiers.
    // The permanent test measures that context and exercises the SDK boundary.
    const MAX_LOCAL_OBSERVATION_BYTES: u32 = 1024 * 1024 + 32 * 1024;
    type Body = <BoardDeliveryTypes as DeliveryTypes>::Body;
    type Sender = <BoardDeliveryTypes as DeliveryTypes>::Sender;
    type Address = <BoardDeliveryTypes as DeliveryTypes>::Address;
    type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;
    type StatusRoute = <BoardDeliveryTypes as DeliveryTypes>::StatusRoute;
    type LocalObservation = RouterLocalObservation;
}

pub struct RouterConsumerProtocol;

impl ConsumerProtocol for RouterConsumerProtocol {
    type Delivery = RouterDeliveryTypes;
    type Request = <BoardConsumerProtocol as ConsumerProtocol>::Request;
    type Answer = <BoardConsumerProtocol as ConsumerProtocol>::Answer;
    type ServerRequest = <BoardConsumerProtocol as ConsumerProtocol>::ServerRequest;
    type ClientAnswer = <BoardConsumerProtocol as ConsumerProtocol>::ClientAnswer;
    type Notice = <BoardConsumerProtocol as ConsumerProtocol>::Notice;
    type Annotation = <BoardConsumerProtocol as ConsumerProtocol>::Annotation;
}

#[cfg(test)]
#[path = "router_delivery_types_tests.rs"]
mod tests;
