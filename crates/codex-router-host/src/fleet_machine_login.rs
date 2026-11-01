//! Host-owned machine login, credential keeping and issuer-token renewal.
mod device_code_flow;
mod issuer_connection;
mod issuer_discovery;
mod issuer_http;
mod login_clock;
mod login_contract;
mod login_owner;
mod login_owner_state;
mod login_publication;
mod login_work;
mod machine_token_transport;
mod refresh_credential_store;
mod startup;
mod token_renewal;
mod token_response;

pub use login_clock::FleetLoginClock;
pub use login_contract::{
    FleetLoginEvent, FleetLoginRefusal, FleetLoginUnknown, RefreshCredentialKeeping, UserCode,
    VerificationUri,
};
pub(crate) use login_owner::{MachineLoginHandle, MachineLoginRoute};
pub(crate) use machine_token_transport::{HostMachineSessionTransport, MachineTokenSource};
pub use startup::FleetLoginDependencies;
pub(crate) use startup::{
    HostMachineLogin, MachineEdgeAccess, MachineLoginBinding, MachineLoginStartup,
    start_machine_login,
};
pub(crate) use token_renewal::MachineRenewalConnection;

#[cfg(test)]
pub(crate) use startup::tests::empty_machine_token_login_fixture;
