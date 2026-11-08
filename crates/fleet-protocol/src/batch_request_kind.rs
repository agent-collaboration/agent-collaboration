use crate::*;
use std::mem::discriminant;

impl<P: ConsumerProtocol> BatchRequest for OperationRequest<P> {
    fn same_request_kind(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Replication(left), Self::Replication(right)) => {
                discriminant(left) == discriminant(right)
            }
            (Self::Lease(left), Self::Lease(right)) => discriminant(left) == discriminant(right),
            (Self::Delivery(left), Self::Delivery(right)) => {
                discriminant(left) == discriminant(right)
            }
            (Self::Consumer(left), Self::Consumer(right)) => P::same_request_kind(left, right),
            (Self::NoticesApplied { .. }, Self::NoticesApplied { .. }) => true,
            _ => false,
        }
    }
}

impl<P: ConsumerProtocol> BatchRequest for ServerRequest<P> {
    fn same_request_kind(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Crate(left), Self::Crate(right)) => discriminant(left) == discriminant(right),
            (Self::Consumer(left), Self::Consumer(right)) => {
                P::same_server_request_kind(left, right)
            }
            _ => false,
        }
    }
}
