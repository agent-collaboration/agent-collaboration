//! Checked native rows become validated login facts at the existing FleetCache owner.
use super::FleetCache;
use crate::{
    fleet_login::{FleetLoginCandidate, SavedFleetCentral, SavedFleetLogin},
    fleet_login_repository::{
        FleetLoginRepository, FleetLoginStorageFailure, FleetLoginStorageRequest,
    },
};
use fleet_identity_protocol::{EnrollmentId, MachineEnvironment};

struct StoredFleetLogin {
    row_id: String,
    enrollment_id: String,
    sign_in: String,
    organization_id: String,
    machine_environment: String,
    receiver_id: String,
    relay_peer_id: String,
    created_at: i64,
    updated_at: i64,
}
fn invalid(field: &'static str) -> FleetLoginStorageFailure {
    FleetLoginStorageFailure::InvalidRecord { field }
}
impl StoredFleetLogin {
    fn decode(&self) -> Result<SavedFleetLogin, FleetLoginStorageFailure> {
        let key = uuid::Uuid::parse_str(&self.row_id).map_err(|_| invalid("row_id"))?;
        if key.get_version_num() != 7
            || key.get_variant() != uuid::Variant::RFC4122
            || key.to_string() != self.row_id
        {
            return Err(invalid("row_id"));
        }
        if self.created_at < 0 || self.updated_at < self.created_at {
            return Err(invalid("timestamps"));
        }
        Ok(SavedFleetLogin {
            enrollment_id: self
                .enrollment_id
                .clone()
                .try_into()
                .map_err(|_| invalid("enrollment_id"))?,
            sign_in: self
                .sign_in
                .clone()
                .try_into()
                .map_err(|_| invalid("sign_in"))?,
            organization_id: self
                .organization_id
                .clone()
                .try_into()
                .map_err(|_| invalid("organization_id"))?,
            machine_environment: match self.machine_environment.as_str() {
                "physical" => MachineEnvironment::Physical,
                "cloudVm" => MachineEnvironment::CloudVm,
                _ => return Err(invalid("machine_environment")),
            },
            central: SavedFleetCentral {
                receiver: self
                    .receiver_id
                    .clone()
                    .try_into()
                    .map_err(|_| invalid("receiver_id"))?,
                relay: self
                    .relay_peer_id
                    .clone()
                    .try_into()
                    .map_err(|_| invalid("relay_peer_id"))?,
            },
        })
    }
}
async fn read_saved(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<Option<StoredFleetLogin>, FleetLoginStorageFailure> {
    let mut rows = sqlx_turso::query_as!(StoredFleetLogin, r#"SELECT row_id AS "row_id!: String",enrollment_id AS "enrollment_id!: String",sign_in AS "sign_in!: String",organization_id AS "organization_id!: String",machine_environment AS "machine_environment!: String",receiver_id AS "receiver_id!: String",relay_peer_id AS "relay_peer_id!: String",created_at AS "created_at!: i64",updated_at AS "updated_at!: i64" FROM fleet_login"#)
        .fetch_all(connection).await.map_err(|_| FleetLoginStorageFailure::Unavailable)?;
    if rows.len() > 1 {
        return Err(invalid("singleton"));
    }
    let row = rows.pop();
    if let Some(row) = &row {
        row.decode()?;
    }
    Ok(row)
}
impl FleetLoginRepository for FleetCache {
    fn load_fleet_login(&self) -> FleetLoginStorageRequest<'_, Option<SavedFleetLogin>> {
        Box::pin(async move {
            let mut connection = self
                .database
                .pool
                .acquire()
                .await
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            read_saved(&mut connection)
                .await?
                .map(|row| row.decode())
                .transpose()
        })
    }
    fn replace_fleet_login<'a>(
        &'a self,
        candidate: &'a FleetLoginCandidate,
    ) -> FleetLoginStorageRequest<'a, SavedFleetLogin> {
        Box::pin(async move {
            let _writer = self.database.writer.lock().await;
            let mut transaction = self
                .database
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            let current = read_saved(&mut transaction).await?;
            let prior = current.as_ref().map(StoredFleetLogin::decode).transpose()?;
            if let Some(prior) = &prior
                && prior.organization_id != candidate.organization_id
            {
                return Err(FleetLoginStorageFailure::OrganizationMismatch {
                    organization_id: prior.organization_id.clone(),
                });
            }
            let now = self.clock.now().unix_millis();
            let (row_id, created_at) = match &current {
                Some(row) => (row.row_id.clone(), row.created_at),
                None => (self.ids.new_id().to_string(), now),
            };
            if now < created_at {
                return Err(invalid("timestamps"));
            }
            let saved = SavedFleetLogin {
                sign_in: candidate.sign_in.clone(),
                organization_id: candidate.organization_id.clone(),
                enrollment_id: candidate.enrollment_id.clone(),
                central: candidate.central.clone(),
                machine_environment: candidate
                    .requested_machine_environment
                    .clone()
                    .or_else(|| prior.map(|login| login.machine_environment))
                    .unwrap_or(MachineEnvironment::Physical),
            };
            let row = StoredFleetLogin {
                row_id,
                enrollment_id: saved.enrollment_id.as_str().to_owned(),
                sign_in: saved.sign_in.as_str().to_owned(),
                organization_id: saved.organization_id.as_str().to_owned(),
                machine_environment: match saved.machine_environment {
                    MachineEnvironment::Physical => "physical",
                    MachineEnvironment::CloudVm => "cloudVm",
                }
                .to_owned(),
                receiver_id: saved.central.receiver.to_string(),
                relay_peer_id: saved.central.relay.to_string(),
                created_at,
                updated_at: now,
            };
            row.decode()?;
            sqlx_turso::query!("DELETE FROM fleet_login")
                .execute(&mut *transaction)
                .await
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            sqlx_turso::query!("INSERT INTO fleet_login(row_id,enrollment_id,sign_in,organization_id,machine_environment,receiver_id,relay_peer_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?)",row.row_id,row.enrollment_id,row.sign_in,row.organization_id,row.machine_environment,row.receiver_id,row.relay_peer_id,row.created_at,row.updated_at)
                .execute(&mut *transaction).await.map_err(|_| FleetLoginStorageFailure::Unavailable)?;
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
            let _writer = self.database.writer.lock().await;
            let mut transaction = self
                .database
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            let expected = expected.as_str();
            let result =
                sqlx_turso::query!("DELETE FROM fleet_login WHERE enrollment_id=?", expected)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            transaction
                .commit()
                .await
                .map_err(|_| FleetLoginStorageFailure::PublicationUnknown)?;
            Ok(result.rows_affected() != 0)
        })
    }
}
