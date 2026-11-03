//! One FleetCache transaction owns each registrar compare-and-replace.
use super::{FleetCache, registration_row::StoredRegistrationRow};
use crate::{
    fleet_session_registration::{
        FleetSessionBindingLifecycle, FleetSessionRegistrationRecord, SessionBindingAddress,
    },
    session_registration_repository::{
        FleetSessionRegistrationRepository, FleetSessionStorageFailure, FleetSessionStorageRequest,
        RegistrationInsert,
    },
};
use collaboration_protocol::OperationId;
use message_board::SessionRef;

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
    connection: &mut sqlx_turso::TursoConnection,
    id: &OperationId,
) -> Result<Option<FleetSessionRegistrationRecord>, FleetSessionStorageFailure> {
    let id = id.as_str();
    sqlx_turso::query_as!(StoredRegistrationRow,r#"SELECT registration_id AS "registration_id!: String",operation_id AS "operation_id!: String",installation_id AS "installation_id!: String",binding_address AS "binding_address!: String",endpoint_service_id AS "endpoint_service_id!: String",endpoint_id AS "endpoint_id!: String",project_id AS "project_id!: String",checked_workspace_id AS "checked_workspace_id!: String",started_by_kind AS "started_by_kind!: String",started_by_service_id AS "started_by_service_id?: String",started_by_endpoint_id AS "started_by_endpoint_id?: String",started_by_session_id AS "started_by_session_id?: String",remote_start_id AS "remote_start_id?: String",isolation_kind AS "isolation_kind!: String",isolation_instance_id AS "isolation_instance_id!: String",working_directory AS "working_directory!: String",repository AS "repository?: String",commit_id AS "commit_id?: String",model AS "model?: String",machine_environment AS "machine_environment!: String",lifecycle_state AS "lifecycle_state!: String",creation_state AS "creation_state?: String",session_service_id AS "session_service_id?: String",session_endpoint_id AS "session_endpoint_id?: String",session_id AS "session_id?: String",organization_id AS "organization_id?: String",abandonment_cause AS "abandonment_cause?: String",refused_setting AS "refused_setting?: String",created_at AS "created_at!: i64",updated_at AS "updated_at!: i64" FROM fleet_session_registrations WHERE operation_id=?"#,id)
        .fetch_optional(connection).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?.map(StoredRegistrationRow::decode).transpose()
}
impl FleetSessionRegistrationRepository for FleetCache {
    fn operation<'a>(
        &'a self,
        id: &'a OperationId,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(async move {
            let mut connection = self
                .database
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
            let id = session.session_id.as_str();
            sqlx_turso::query_as!(StoredRegistrationRow,r#"SELECT registration_id AS "registration_id!: String",operation_id AS "operation_id!: String",installation_id AS "installation_id!: String",binding_address AS "binding_address!: String",endpoint_service_id AS "endpoint_service_id!: String",endpoint_id AS "endpoint_id!: String",project_id AS "project_id!: String",checked_workspace_id AS "checked_workspace_id!: String",started_by_kind AS "started_by_kind!: String",started_by_service_id AS "started_by_service_id?: String",started_by_endpoint_id AS "started_by_endpoint_id?: String",started_by_session_id AS "started_by_session_id?: String",remote_start_id AS "remote_start_id?: String",isolation_kind AS "isolation_kind!: String",isolation_instance_id AS "isolation_instance_id!: String",working_directory AS "working_directory!: String",repository AS "repository?: String",commit_id AS "commit_id?: String",model AS "model?: String",machine_environment AS "machine_environment!: String",lifecycle_state AS "lifecycle_state!: String",creation_state AS "creation_state?: String",session_service_id AS "session_service_id?: String",session_endpoint_id AS "session_endpoint_id?: String",session_id AS "session_id?: String",organization_id AS "organization_id?: String",abandonment_cause AS "abandonment_cause?: String",refused_setting AS "refused_setting?: String",created_at AS "created_at!: i64",updated_at AS "updated_at!: i64" FROM fleet_session_registrations WHERE session_service_id=? AND session_endpoint_id=? AND session_id=?"#,service,endpoint,id)
                .fetch_optional(&self.database.pool).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?.map(StoredRegistrationRow::decode).transpose()
        })
    }
    fn binding<'a>(
        &'a self,
        address: &'a SessionBindingAddress,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(async move {
            let address = address.expose_address();
            sqlx_turso::query_as!(StoredRegistrationRow,r#"SELECT registration_id AS "registration_id!: String",operation_id AS "operation_id!: String",installation_id AS "installation_id!: String",binding_address AS "binding_address!: String",endpoint_service_id AS "endpoint_service_id!: String",endpoint_id AS "endpoint_id!: String",project_id AS "project_id!: String",checked_workspace_id AS "checked_workspace_id!: String",started_by_kind AS "started_by_kind!: String",started_by_service_id AS "started_by_service_id?: String",started_by_endpoint_id AS "started_by_endpoint_id?: String",started_by_session_id AS "started_by_session_id?: String",remote_start_id AS "remote_start_id?: String",isolation_kind AS "isolation_kind!: String",isolation_instance_id AS "isolation_instance_id!: String",working_directory AS "working_directory!: String",repository AS "repository?: String",commit_id AS "commit_id?: String",model AS "model?: String",machine_environment AS "machine_environment!: String",lifecycle_state AS "lifecycle_state!: String",creation_state AS "creation_state?: String",session_service_id AS "session_service_id?: String",session_endpoint_id AS "session_endpoint_id?: String",session_id AS "session_id?: String",organization_id AS "organization_id?: String",abandonment_cause AS "abandonment_cause?: String",refused_setting AS "refused_setting?: String",created_at AS "created_at!: i64",updated_at AS "updated_at!: i64" FROM fleet_session_registrations WHERE binding_address=?"#,address)
                .fetch_optional(&self.database.pool).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?.map(StoredRegistrationRow::decode).transpose()
        })
    }
    fn insert<'a>(
        &'a self,
        record: &'a FleetSessionRegistrationRecord,
    ) -> FleetSessionStorageRequest<'a, RegistrationInsert> {
        Box::pin(async move {
            let _writer = self.database.writer.lock().await;
            let mut transaction = self
                .database
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?;
            if let Some(existing) =
                operation(&mut transaction, &record.preparation().operation_id).await?
            {
                return Ok(RegistrationInsert::Existing(Box::new(existing)));
            }
            let now = self.clock.now().unix_millis();
            let row = StoredRegistrationRow::encode(record, now, now)?;
            sqlx_turso::query!("INSERT INTO fleet_session_registrations(registration_id,operation_id,installation_id,binding_address,endpoint_service_id,endpoint_id,project_id,checked_workspace_id,started_by_kind,started_by_service_id,started_by_endpoint_id,started_by_session_id,remote_start_id,isolation_kind,isolation_instance_id,working_directory,repository,commit_id,model,machine_environment,lifecycle_state,creation_state,session_service_id,session_endpoint_id,session_id,organization_id,abandonment_cause,refused_setting,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",row.registration_id,row.operation_id,row.installation_id,row.binding_address,row.endpoint_service_id,row.endpoint_id,row.project_id,row.checked_workspace_id,row.started_by_kind,row.started_by_service_id,row.started_by_endpoint_id,row.started_by_session_id,row.remote_start_id,row.isolation_kind,row.isolation_instance_id,row.working_directory,row.repository,row.commit_id,row.model,row.machine_environment,row.lifecycle_state,row.creation_state,row.session_service_id,row.session_endpoint_id,row.session_id,row.organization_id,row.abandonment_cause,row.refused_setting,row.created_at,row.updated_at)
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
            let _writer = self.database.writer.lock().await;
            let mut transaction = self
                .database
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?;
            let id = expected.preparation().registration_id.as_str();
            let current=sqlx_turso::query_as!(StoredRegistrationRow,r#"SELECT registration_id AS "registration_id!: String",operation_id AS "operation_id!: String",installation_id AS "installation_id!: String",binding_address AS "binding_address!: String",endpoint_service_id AS "endpoint_service_id!: String",endpoint_id AS "endpoint_id!: String",project_id AS "project_id!: String",checked_workspace_id AS "checked_workspace_id!: String",started_by_kind AS "started_by_kind!: String",started_by_service_id AS "started_by_service_id?: String",started_by_endpoint_id AS "started_by_endpoint_id?: String",started_by_session_id AS "started_by_session_id?: String",remote_start_id AS "remote_start_id?: String",isolation_kind AS "isolation_kind!: String",isolation_instance_id AS "isolation_instance_id!: String",working_directory AS "working_directory!: String",repository AS "repository?: String",commit_id AS "commit_id?: String",model AS "model?: String",machine_environment AS "machine_environment!: String",lifecycle_state AS "lifecycle_state!: String",creation_state AS "creation_state?: String",session_service_id AS "session_service_id?: String",session_endpoint_id AS "session_endpoint_id?: String",session_id AS "session_id?: String",organization_id AS "organization_id?: String",abandonment_cause AS "abandonment_cause?: String",refused_setting AS "refused_setting?: String",created_at AS "created_at!: i64",updated_at AS "updated_at!: i64" FROM fleet_session_registrations WHERE registration_id=?"#,id)
                .fetch_optional(&mut *transaction).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?;
            let Some(current) = current else {
                return Ok(false);
            };
            let created_at = current.created_at;
            if current.decode()? != *expected {
                return Ok(false);
            }
            let row =
                StoredRegistrationRow::encode(next, created_at, self.clock.now().unix_millis())?;
            let written=sqlx_turso::query!("UPDATE fleet_session_registrations SET operation_id=?,installation_id=?,binding_address=?,endpoint_service_id=?,endpoint_id=?,project_id=?,checked_workspace_id=?,started_by_kind=?,started_by_service_id=?,started_by_endpoint_id=?,started_by_session_id=?,remote_start_id=?,isolation_kind=?,isolation_instance_id=?,working_directory=?,repository=?,commit_id=?,model=?,machine_environment=?,lifecycle_state=?,creation_state=?,session_service_id=?,session_endpoint_id=?,session_id=?,organization_id=?,abandonment_cause=?,refused_setting=?,created_at=?,updated_at=? WHERE registration_id=?",row.operation_id,row.installation_id,row.binding_address,row.endpoint_service_id,row.endpoint_id,row.project_id,row.checked_workspace_id,row.started_by_kind,row.started_by_service_id,row.started_by_endpoint_id,row.started_by_session_id,row.remote_start_id,row.isolation_kind,row.isolation_instance_id,row.working_directory,row.repository,row.commit_id,row.model,row.machine_environment,row.lifecycle_state,row.creation_state,row.session_service_id,row.session_endpoint_id,row.session_id,row.organization_id,row.abandonment_cause,row.refused_setting,row.created_at,row.updated_at,id)
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
            let rows=sqlx_turso::query_as!(StoredRegistrationRow,r#"SELECT registration_id AS "registration_id!: String",operation_id AS "operation_id!: String",installation_id AS "installation_id!: String",binding_address AS "binding_address!: String",endpoint_service_id AS "endpoint_service_id!: String",endpoint_id AS "endpoint_id!: String",project_id AS "project_id!: String",checked_workspace_id AS "checked_workspace_id!: String",started_by_kind AS "started_by_kind!: String",started_by_service_id AS "started_by_service_id?: String",started_by_endpoint_id AS "started_by_endpoint_id?: String",started_by_session_id AS "started_by_session_id?: String",remote_start_id AS "remote_start_id?: String",isolation_kind AS "isolation_kind!: String",isolation_instance_id AS "isolation_instance_id!: String",working_directory AS "working_directory!: String",repository AS "repository?: String",commit_id AS "commit_id?: String",model AS "model?: String",machine_environment AS "machine_environment!: String",lifecycle_state AS "lifecycle_state!: String",creation_state AS "creation_state?: String",session_service_id AS "session_service_id?: String",session_endpoint_id AS "session_endpoint_id?: String",session_id AS "session_id?: String",organization_id AS "organization_id?: String",abandonment_cause AS "abandonment_cause?: String",refused_setting AS "refused_setting?: String",created_at AS "created_at!: i64",updated_at AS "updated_at!: i64" FROM fleet_session_registrations ORDER BY registration_id"#)
                .fetch_all(&self.database.pool).await.map_err(|_|FleetSessionStorageFailure::Unavailable)?;
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
