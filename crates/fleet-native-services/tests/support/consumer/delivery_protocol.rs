use fleet_mail_delivery::{
    DeliveryConfigError, DeliveryLimits, DeliveryTypes, minimum_settlement_allowance,
};

pub struct SampleDelivery;
impl DeliveryTypes for SampleDelivery {
    const MAX_SENDER_BYTES: u32 = 128;
    const MAX_ADDRESS_BYTES: u32 = 128;
    const MAX_ACCEPTANCE_BYTES: u32 = 256;
    const MAX_STATUS_ROUTE_BYTES: u32 = 128;
    type Body = String;
    type Sender = String;

    type Address = String;
    type Acceptance = String;
    type LocalObservation = ();
    const MAX_LOCAL_OBSERVATION_BYTES: u32 = 4;
    type StatusRoute = String;
}
pub fn delivery_limits() -> Result<DeliveryLimits, DeliveryConfigError> {
    DeliveryLimits::new::<SampleDelivery>(
        10_000_000,
        200,
        10_000_000,
        minimum_settlement_allowance::<SampleDelivery>()?,
    )
}

// This private fixture source names its producer independently of the transport claim.
pub(crate) struct SampleSenderBinding;
impl<S: durable_steps::storage::TransactionScope>
    fleet_mail_delivery::SenderBinding<S, SampleDelivery> for SampleSenderBinding
{
    async fn resolve_submitter<'a>(
        &'a self,
        _: &'a mut S,
        sender: &'a String,
    ) -> Result<fleet_mail_delivery::Submitter, fleet_mail_delivery::SenderBindingFailure> {
        let peer = sender
            .strip_suffix("|authorized sender")
            .ok_or(fleet_mail_delivery::SenderBindingFailure::Unknown)?;
        fleet_mail_delivery::PeerId::try_from(peer.to_owned())
            .map(fleet_mail_delivery::Submitter::Machine)
            .map_err(|_| fleet_mail_delivery::SenderBindingFailure::Unknown)
    }
}
