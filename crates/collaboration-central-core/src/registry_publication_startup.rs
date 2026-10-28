//! A cached runtime requests startup discovery once, with cancellation-safe retry.
use durable_steps::StoreError;
use std::sync::Mutex;

#[derive(Default)]
pub(crate) struct PublicationStartup(Mutex<StartupState>);
#[derive(Default)]
enum StartupState {
    #[default]
    Pending,
    Running,
    Complete,
}
pub(crate) struct StartupDiscovery<'state> {
    startup: &'state PublicationStartup,
}
impl PublicationStartup {
    pub fn begin(&self) -> Result<Option<StartupDiscovery<'_>>, StoreError> {
        let mut state = self.0.lock().map_err(|_| unavailable())?;
        match *state {
            StartupState::Pending => {
                *state = StartupState::Running;
                Ok(Some(StartupDiscovery { startup: self }))
            }
            StartupState::Running | StartupState::Complete => Ok(None),
        }
    }
}
impl StartupDiscovery<'_> {
    pub fn complete(self) -> Result<(), StoreError> {
        *self.startup.0.lock().map_err(|_| unavailable())? = StartupState::Complete;
        Ok(())
    }
}
impl Drop for StartupDiscovery<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.startup.0.lock()
            && matches!(*state, StartupState::Running)
        {
            *state = StartupState::Pending;
        }
    }
}
fn unavailable() -> StoreError {
    StoreError::Aborted {
        reason: "Registry startup discovery is unavailable".into(),
    }
}
