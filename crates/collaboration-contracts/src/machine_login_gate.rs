//! The board application asks the Host's current machine-login state before admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineLoginState {
    LoggedIn,
    NotLoggedIn,
}

pub trait MachineLoginGate: Send + Sync {
    fn machine_login_state(&self) -> MachineLoginState;
}
