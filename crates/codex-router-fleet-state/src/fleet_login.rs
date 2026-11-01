//! Public routing facts for one confirmed enrollment; credentials have another owner.
use fleet_identity_protocol::{EnrollmentId, MachineEnvironment, OrganizationId};
use fleet_sync_protocol::{PeerId, ReceiverId};

mod sign_in_address;
pub use sign_in_address::SignInAddress;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedFleetLogin {
    pub sign_in: SignInAddress,
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
    pub machine_environment: MachineEnvironment,
    pub central: SavedFleetCentral,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedFleetCentral {
    pub receiver: ReceiverId,
    pub relay: PeerId,
}
