//! Checked SQL rows become validated login facts at this repository boundary.
use crate::{
    fleet_login::{FleetLoginCandidate, SavedFleetCentral, SavedFleetLogin},
    fleet_login_repository::{
        FleetLoginRepository, FleetLoginStorageFailure, FleetLoginStorageRequest,
    },
    sqlite::FleetStateStore,
};
use fleet_identity_protocol::{EnrollmentId, MachineEnvironment};

struct StoredFleetLogin {
    enrollment_id: String,
    sign_in: String,
    organization_id: String,
    machine_environment: String,
    receiver_id: String,
    relay_peer_id: String,
}
fn invalid(field: &'static str) -> FleetLoginStorageFailure {
    FleetLoginStorageFailure::InvalidRecord { field }
}
impl TryFrom<StoredFleetLogin> for SavedFleetLogin {
    type Error = FleetLoginStorageFailure;
    fn try_from(row: StoredFleetLogin) -> Result<Self, Self::Error> {
        Ok(Self {
            enrollment_id: row
                .enrollment_id
                .try_into()
                .map_err(|_| invalid("enrollment_id"))?,
            sign_in: row.sign_in.try_into().map_err(|_| invalid("sign_in"))?,
            organization_id: row
                .organization_id
                .try_into()
                .map_err(|_| invalid("organization_id"))?,
            machine_environment: match row.machine_environment.as_str() {
                "physical" => MachineEnvironment::Physical,
                "cloudVm" => MachineEnvironment::CloudVm,
                _ => return Err(invalid("machine_environment")),
            },
            central: SavedFleetCentral {
                receiver: row
                    .receiver_id
                    .try_into()
                    .map_err(|_| invalid("receiver_id"))?,
                relay: row
                    .relay_peer_id
                    .try_into()
                    .map_err(|_| invalid("relay_peer_id"))?,
            },
        })
    }
}
async fn read_saved(
    connection: &mut sqlx::SqliteConnection,
) -> Result<Option<SavedFleetLogin>, FleetLoginStorageFailure> {
    let mut rows = sqlx::query_as!(StoredFleetLogin, "SELECT enrollment_id,sign_in,organization_id,machine_environment,receiver_id,relay_peer_id FROM fleet_login")
        .fetch_all(connection).await.map_err(|_|FleetLoginStorageFailure::Unavailable)?;
    if rows.len() > 1 {
        return Err(invalid("singleton"));
    }
    rows.pop().map(TryInto::try_into).transpose()
}
impl FleetLoginRepository for FleetStateStore {
    fn load_fleet_login(&self) -> FleetLoginStorageRequest<'_, Option<SavedFleetLogin>> {
        Box::pin(async move {
            let mut connection = self
                .pool
                .acquire()
                .await
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            read_saved(&mut connection).await
        })
    }
    fn replace_fleet_login<'a>(
        &'a self,
        candidate: &'a FleetLoginCandidate,
    ) -> FleetLoginStorageRequest<'a, SavedFleetLogin> {
        Box::pin(async move {
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            let current = read_saved(&mut transaction).await?;
            if let Some(current) = &current
                && current.organization_id != candidate.organization_id
            {
                return Err(FleetLoginStorageFailure::OrganizationMismatch {
                    organization_id: current.organization_id.clone(),
                });
            }
            let saved = SavedFleetLogin {
                sign_in: candidate.sign_in.clone(),
                organization_id: candidate.organization_id.clone(),
                enrollment_id: candidate.enrollment_id.clone(),
                machine_environment: candidate
                    .requested_machine_environment
                    .clone()
                    .or_else(|| current.map(|saved| saved.machine_environment))
                    .unwrap_or(MachineEnvironment::Physical),
                central: candidate.central.clone(),
            };
            let enrollment = saved.enrollment_id.as_str();
            let sign_in = saved.sign_in.as_str();
            let organization = saved.organization_id.as_str();
            let environment = match saved.machine_environment {
                MachineEnvironment::Physical => "physical",
                MachineEnvironment::CloudVm => "cloudVm",
            };
            let receiver = saved.central.receiver.to_string();
            let relay = saved.central.relay.to_string();
            sqlx::query!("DELETE FROM fleet_login")
                .execute(&mut *transaction)
                .await
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            sqlx::query!("INSERT INTO fleet_login(enrollment_id,sign_in,organization_id,machine_environment,receiver_id,relay_peer_id) VALUES (?1,?2,?3,?4,?5,?6)",enrollment,sign_in,organization,environment,receiver,relay)
                .execute(&mut *transaction).await.map_err(|_|FleetLoginStorageFailure::Unavailable)?;
            transaction
                .commit()
                .await
                .map_err(|_| FleetLoginStorageFailure::PublicationUnknown)?;
            Ok(saved)
        })
    }
    fn delete_fleet_login<'a>(
        &'a self,
        expected: &'a EnrollmentId,
    ) -> FleetLoginStorageRequest<'a, bool> {
        Box::pin(async move {
            let expected = expected.as_str();
            let result = sqlx::query!("DELETE FROM fleet_login WHERE enrollment_id=?1", expected)
                .execute(&self.pool)
                .await
                .map_err(|_| FleetLoginStorageFailure::PublicationUnknown)?;
            Ok(result.rows_affected() != 0)
        })
    }
}
