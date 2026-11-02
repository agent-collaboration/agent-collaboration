//! Only a validated whole envelope may supply a registrar row's indexed projections.
use crate::{
    fleet_session_registration::{
        FleetSessionBindingLifecycle, FleetSessionRegistrationRecord, SessionBindingAddress,
    },
    session_registration_repository::{
        FleetSessionRegistrationRepository, FleetSessionStorageFailure, FleetSessionStorageRequest,
        RegistrationInsert,
    },
    sqlite::FleetStateStore,
};
use collaboration_protocol::OperationId;
use message_board::SessionRef;

struct StoredRegistrationRow {
    registration_id: String,
    operation_id: String,
    installation_id: String,
    binding_address: String,
    session_service_id: Option<String>,
    session_endpoint_id: Option<String>,
    session_id: Option<String>,
    lifecycle_json: String,
}

impl StoredRegistrationRow {
    fn encode(record: &FleetSessionRegistrationRecord) -> Result<Self, FleetSessionStorageFailure> {
        let record = FleetSessionRegistrationRecord::new(
            record.preparation().clone(),
            record.lifecycle().clone(),
        )
        .map_err(|_| FleetSessionStorageFailure::InvalidRecord)?;
        let preparation = record.preparation();
        let session = record.lifecycle().session();
        Ok(Self {
            registration_id: preparation.registration_id.as_str().to_owned(),
            operation_id: preparation.operation_id.as_str().to_owned(),
            installation_id: preparation.installation.as_str().to_owned(),
            binding_address: preparation.binding_address.expose_address().to_owned(),
            session_service_id: session
                .map(|session| session.endpoint.service_id.as_str().to_owned()),
            session_endpoint_id: session
                .map(|session| session.endpoint.endpoint_id.as_str().to_owned()),
            session_id: session.map(|session| session.session_id.as_str().to_owned()),
            lifecycle_json: serde_json::to_string(&record)
                .map_err(|_| FleetSessionStorageFailure::InvalidRecord)?,
        })
    }
    fn decode(self) -> Result<FleetSessionRegistrationRecord, FleetSessionStorageFailure> {
        let record: FleetSessionRegistrationRecord = serde_json::from_str(&self.lifecycle_json)
            .map_err(|_| FleetSessionStorageFailure::InvalidRecord)?;
        let preparation = record.preparation();
        let session = record.lifecycle().session();
        if preparation.registration_id.as_str() != self.registration_id
            || preparation.operation_id.as_str() != self.operation_id
            || preparation.installation.as_str() != self.installation_id
            || preparation.binding_address.expose_address() != self.binding_address
            || session.map(|value| value.endpoint.service_id.as_str())
                != self.session_service_id.as_deref()
            || session.map(|value| value.endpoint.endpoint_id.as_str())
                != self.session_endpoint_id.as_deref()
            || session.map(|value| value.session_id.as_str()) != self.session_id.as_deref()
        {
            return Err(FleetSessionStorageFailure::InvalidRecord);
        }
        Ok(record)
    }
}

fn write_failure(error: sqlx::Error) -> FleetSessionStorageFailure {
    if error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation())
    {
        FleetSessionStorageFailure::Conflict
    } else {
        FleetSessionStorageFailure::Unavailable
    }
}

async fn operation(
    connection: &mut sqlx::SqliteConnection,
    operation: &OperationId,
) -> Result<Option<FleetSessionRegistrationRecord>, FleetSessionStorageFailure> {
    let operation = operation.as_str();
    sqlx::query_as!(StoredRegistrationRow,"SELECT registration_id,operation_id,installation_id,binding_address,session_service_id,session_endpoint_id,session_id,lifecycle_json FROM fleet_session_registrations WHERE operation_id=?1",operation)
        .fetch_optional(connection).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?
        .map(StoredRegistrationRow::decode).transpose()
}

impl FleetSessionRegistrationRepository for FleetStateStore {
    fn operation<'a>(
        &'a self,
        id: &'a OperationId,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(async move {
            let mut connection = self
                .pool
                .acquire()
                .await
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?;
            operation(&mut connection, id).await
        })
    }
    fn conversation<'a>(
        &'a self,
        session: &'a SessionRef,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(async move {
            let service = session.endpoint.service_id.as_str();
            let endpoint = session.endpoint.endpoint_id.as_str();
            let session = session.session_id.as_str();
            sqlx::query_as!(StoredRegistrationRow,"SELECT registration_id,operation_id,installation_id,binding_address,session_service_id,session_endpoint_id,session_id,lifecycle_json FROM fleet_session_registrations WHERE session_service_id=?1 AND session_endpoint_id=?2 AND session_id=?3",service,endpoint,session)
                .fetch_optional(&self.pool).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?
                .map(StoredRegistrationRow::decode).transpose()
        })
    }
    fn binding<'a>(
        &'a self,
        address: &'a SessionBindingAddress,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(async move {
            let address = address.expose_address();
            sqlx::query_as!(StoredRegistrationRow,"SELECT registration_id,operation_id,installation_id,binding_address,session_service_id,session_endpoint_id,session_id,lifecycle_json FROM fleet_session_registrations WHERE binding_address=?1",address)
                .fetch_optional(&self.pool).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?
                .map(StoredRegistrationRow::decode).transpose()
        })
    }
    fn insert<'a>(
        &'a self,
        record: &'a FleetSessionRegistrationRecord,
    ) -> FleetSessionStorageRequest<'a, RegistrationInsert> {
        Box::pin(async move {
            let row = StoredRegistrationRow::encode(record)?;
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?;
            if let Some(existing) =
                operation(&mut transaction, &record.preparation().operation_id).await?
            {
                return Ok(RegistrationInsert::Existing(Box::new(existing)));
            }
            sqlx::query!("INSERT INTO fleet_session_registrations(registration_id,operation_id,installation_id,binding_address,session_service_id,session_endpoint_id,session_id,lifecycle_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",row.registration_id,row.operation_id,row.installation_id,row.binding_address,row.session_service_id,row.session_endpoint_id,row.session_id,row.lifecycle_json)
                .execute(&mut *transaction).await.map_err(write_failure)?;
            transaction
                .commit()
                .await
                .map_err(|_| FleetSessionStorageFailure::PublicationUnknown)?;
            Ok(RegistrationInsert::Inserted)
        })
    }
    fn replace<'a>(
        &'a self,
        expected: &'a FleetSessionRegistrationRecord,
        next: &'a FleetSessionRegistrationRecord,
    ) -> FleetSessionStorageRequest<'a, bool> {
        Box::pin(async move {
            if expected.preparation() != next.preparation() {
                return Ok(false);
            }
            let row = StoredRegistrationRow::encode(next)?;
            let id = expected.preparation().registration_id.as_str();
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?;
            let current=sqlx::query_as!(StoredRegistrationRow,"SELECT registration_id,operation_id,installation_id,binding_address,session_service_id,session_endpoint_id,session_id,lifecycle_json FROM fleet_session_registrations WHERE registration_id=?1",id)
                .fetch_optional(&mut *transaction).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?
                .map(StoredRegistrationRow::decode).transpose()?;
            if current.as_ref() != Some(expected) {
                return Ok(false);
            }
            let written=sqlx::query!("UPDATE fleet_session_registrations SET session_service_id=?1,session_endpoint_id=?2,session_id=?3,lifecycle_json=?4 WHERE registration_id=?5",row.session_service_id,row.session_endpoint_id,row.session_id,row.lifecycle_json,id)
                .execute(&mut *transaction).await.map_err(write_failure)?;
            transaction
                .commit()
                .await
                .map_err(|_| FleetSessionStorageFailure::PublicationUnknown)?;
            Ok(written.rows_affected() == 1)
        })
    }
    fn reconcile_candidates(
        &self,
    ) -> FleetSessionStorageRequest<'_, Vec<FleetSessionRegistrationRecord>> {
        Box::pin(async move {
            let rows=sqlx::query_as!(StoredRegistrationRow,"SELECT registration_id,operation_id,installation_id,binding_address,session_service_id,session_endpoint_id,session_id,lifecycle_json FROM fleet_session_registrations ORDER BY registration_id")
                .fetch_all(&self.pool).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?;
            let mut candidates = Vec::new();
            for row in rows {
                let record = row.decode()?;
                if matches!(
                    record.lifecycle(),
                    FleetSessionBindingLifecycle::Prepared { .. }
                        | FleetSessionBindingLifecycle::Active { .. }
                ) {
                    candidates.push(record);
                }
            }
            Ok(candidates)
        })
    }
}
