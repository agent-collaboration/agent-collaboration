//! Supplies the engine alarm query before a cold central object has a runtime.
use central_storage::{AlarmUpdate, StoreFailure};
use fleet_contracts::AlarmRecompute;
use fleet_runtime_ports::CentralStore;
use fleet_signin::CentralOrganizationIdentity;

pub(crate) struct SignInAlarmPolicy;

impl AlarmRecompute for SignInAlarmPolicy {
    fn alarm_update(&self) -> AlarmUpdate {
        fleet_engine::alarm_update()
    }
}

pub async fn prepare_central_alarm(
    host: &impl CentralStore,
) -> Result<CentralOrganizationIdentity, StoreFailure> {
    fleet_signin::prepare_central_alarm(host, &SignInAlarmPolicy).await
}
