//! Surface codecs translate names while retaining the library's typed payloads.

use crate::surface_codecs::{
    CancelResultCodec, ClaimAnswerCodec, DeliveryStatusCodec, LeaseRefusalCodec, PieceRefusalCodec,
    RegistrationRefusalCodec,
};
use fleet_sync_protocol as wire;
use serde::{Deserialize, Serialize};

macro_rules! surface_wrapper {
    ($($name:ident($source:ty) => $codec:literal;)+) => {$(
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(#[serde(with = $codec)] pub $source);

        impl From<$source> for $name {
            fn from(value: $source) -> Self { Self(value) }
        }

        impl From<$name> for $source {
            fn from(value: $name) -> Self { value.0 }
        }
    )+};
}

surface_wrapper! {
    BoardPieceRefusal(wire::PieceRefusal) => "PieceRefusalCodec";
    BoardLeaseRefusal(wire::LeaseRefusal) => "LeaseRefusalCodec";
    BoardRegistrationRefusal(wire::RegistrationRefusal) => "RegistrationRefusalCodec";
    BoardClaimAnswer(wire::HolderChangeAnswer) => "ClaimAnswerCodec";
    BoardDeliveryStatus(wire::DeliveryStatus<collaboration_mail_protocol::CarrierAcceptance>) => "DeliveryStatusCodec";
    BoardCancelResult(wire::CancelResult) => "CancelResultCodec";
}
