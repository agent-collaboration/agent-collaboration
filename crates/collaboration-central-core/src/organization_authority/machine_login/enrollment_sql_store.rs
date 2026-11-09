//! `MachineEnrollmentPort` over an organization's real Fleet store: one store
//! transaction reads the facts, runs the decision and applies its changes, or
//! commits nothing. One generic implementation serves both hosts: Turso under
//! Tokio and Durable Object SQLite under workerd.
//!
//! It reads the staged central-registry v3/v4 tables, which are not registered
//! in the live corpus yet (until the B3 reader slice).
use super::enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, FormerServiceFacts, InstallationRow, MachineClaim,
    StoredEnrollment,
};
use super::enrollment_sql_codec::{
    allowance_state_text, decode_allowance, decode_enrollment, decode_installation, encode_state,
    generation_text, millis_of_seconds,
};
use crate::fleet_row_codec::corrupt;
use crate::generated_b3_registry as queries;
use crate::organization_authority::change_records;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_identity_protocol::OrganizationId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::marker::PhantomData;
use std::sync::Arc;

/// The Fleet store of one organization, as the machine-login port.
pub struct SqlEnrollmentPort<Store, Driver> {
    store: Arc<Store>,
    organization: OrganizationId,
    driver: PhantomData<fn() -> Driver>,
}

impl<Store, Driver> SqlEnrollmentPort<Store, Driver> {
    #[must_use]
    pub fn new(store: Arc<Store>, organization: OrganizationId) -> Self {
        Self {
            store,
            organization,
            driver: PhantomData,
        }
    }
}

/// What a transaction found before it could decide.
enum Prepared {
    Facts(Box<EnrollmentFacts>),
    /// The request ID was seen before, and this store keeps no request body to
    /// compare a repeat with (pending the owner's fingerprint decision).
    RepeatPendingOwner,
}

impl<Store, Driver> MachineEnrollmentPort for SqlEnrollmentPort<Store, Driver>
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    async fn transact<T: Send + 'static>(
        &self,
        query: EnrollmentQuery,
        decide: impl FnOnce(&EnrollmentFacts) -> Decided<T> + Send + 'static,
    ) -> Result<T, MachineEnrollmentFailure> {
        let organization = self.organization.clone();
        let outcome = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    match read_facts(scope, &query, organization).await? {
                        Prepared::RepeatPendingOwner => {
                            Ok(Err(MachineEnrollmentFailure::RepeatPendingOwner))
                        }
                        Prepared::Facts(facts) => {
                            let decided = decide(&facts);
                            apply_changes(scope, &decided.changes, decided.written_at_ms).await?;
                            Ok(Ok(decided.answer))
                        }
                    }
                })
            })
            .await
            .map_err(store_failure)?;
        outcome
    }
}

fn store_failure(error: StoreError) -> MachineEnrollmentFailure {
    match error {
        StoreError::Corrupt { .. } => MachineEnrollmentFailure::CorruptStore,
        _ => MachineEnrollmentFailure::Unavailable,
    }
}

fn sql(error: sqlx_core::Error) -> StoreError {
    StoreError::backend(error)
}

async fn read_facts<S>(
    scope: &mut S,
    query: &EnrollmentQuery,
    organization: OrganizationId,
) -> Result<Prepared, StoreError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    if let Some(request) = &query.request
        && queries::request_seen(scope, request.as_str().to_owned())
            .await
            .map_err(sql)?
    {
        return Ok(Prepared::RepeatPendingOwner);
    }
    let organization_has_enrollment = queries::any_enrollment(scope).await.map_err(sql)?;
    let enrollment = match &query.enrollment {
        Some(id) => queries::read_enrollment(scope, id.as_str().to_owned())
            .await
            .map_err(sql)?
            .map(decode_enrollment)
            .transpose()?,
        None => None,
    };
    let installation_key = query
        .installation
        .clone()
        .or_else(|| enrollment.as_ref().map(|found| found.installation.clone()));
    let mut installation = None;
    let mut active = None;
    let mut allowances = Vec::new();
    if let Some(key) = &installation_key {
        active = queries::read_enrolled_enrollment(scope, key.as_str().to_owned())
            .await
            .map_err(sql)?
            .map(decode_enrollment)
            .transpose()?;
        installation = queries::read_installation(scope, key.as_str().to_owned())
            .await
            .map_err(sql)?
            .map(decode_installation)
            .transpose()?
            .map(|row| InstallationRow {
                installation: row.id,
                label: row.label,
                generation: row.generation,
                active: active
                    .as_ref()
                    .map(|enrolled: &StoredEnrollment| enrolled.id.clone()),
            });
        allowances = queries::read_allowances(scope, key.as_str().to_owned())
            .await
            .map_err(sql)?
            .into_iter()
            .map(decode_allowance)
            .collect::<Result<Vec<_>, _>>()?;
    }
    let mut missing_workspaces = Vec::new();
    for workspace in &query.workspaces {
        if crate::generated_fleet_workspaces::read_workspace_columns(
            scope,
            workspace.as_str().to_owned(),
        )
        .await
        .map_err(sql)?
        .is_none()
        {
            missing_workspaces.push(workspace.clone());
        }
    }
    let former = match &query.former {
        None => FormerServiceFacts::NotAsked,
        Some(former) => {
            match queries::read_installation_by_former(
                scope,
                former.as_service_id().as_str().to_owned(),
            )
            .await
            .map_err(sql)?
            .map(decode_installation)
            .transpose()?
            {
                Some(linked) => FormerServiceFacts::LinkedTo {
                    installation: linked.id,
                },
                None => FormerServiceFacts::NamesNothing,
            }
        }
    };
    // Each organization's store sees only itself; claims across organizations
    // are deferred (B3 doc).
    let claim = if installation.is_some() {
        MachineClaim::ThisOrganization
    } else {
        MachineClaim::Unclaimed
    };
    Ok(Prepared::Facts(Box::new(EnrollmentFacts {
        organization,
        organization_has_enrollment,
        prior_approval: None,
        enrollment,
        installation,
        active,
        allowances,
        claim,
        missing_workspaces,
        former,
    })))
}

async fn apply_changes<S>(
    scope: &mut S,
    changes: &[EnrollmentChange],
    written_at_ms: i64,
) -> Result<(), StoreError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Decode<'value, S::Database> + Encode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let at = written_at_ms.to_string();
    for change in changes {
        match change {
            EnrollmentChange::InsertEnrollment(enrollment) => {
                let encoded = encode_state(&enrollment.state)?;
                queries::insert_enrollment(
                    scope,
                    enrollment.id.as_str().to_owned(),
                    enrollment.installation.as_str().to_owned(),
                    enrollment.request_id.as_str().to_owned(),
                    encoded.state.to_owned(),
                    encoded.end_reason,
                    enrollment.approved_by.as_str().to_owned(),
                    "machine:enroll".to_owned(),
                    generation_text(
                        enrollment.base_generation,
                        "machine_enrollments.base_generation",
                    )?,
                    millis_of_seconds(
                        enrollment.code_expires_at,
                        "machine_enrollments.code_expires_at",
                    )?
                    .to_string(),
                    encoded.machine_client_id,
                    encoded.last_grant_at,
                    enrollment.ended_at_ms.map(|at| at.to_string()),
                    enrollment.approved_at_ms.to_string(),
                    at.clone(),
                )
                .await
                .map_err(sql)?;
            }
            EnrollmentChange::ReplaceEnrollment(enrollment) => {
                let stored = queries::read_enrollment(scope, enrollment.id.as_str().to_owned())
                    .await
                    .map_err(sql)?
                    .map(decode_enrollment)
                    .transpose()?
                    .ok_or_else(|| {
                        corrupt("machine_enrollments.id", "a replaced enrollment is absent")
                    })?;
                if !same_identity(&stored, enrollment) {
                    return Err(corrupt(
                        "machine_enrollments",
                        "a transition changes only state, client, last Grant and end",
                    ));
                }
                let encoded = encode_state(&enrollment.state)?;
                queries::update_enrollment(
                    scope,
                    encoded.state.to_owned(),
                    encoded.end_reason,
                    encoded.machine_client_id,
                    encoded.last_grant_at,
                    enrollment.ended_at_ms.map(|at| at.to_string()),
                    at.clone(),
                    enrollment.id.as_str().to_owned(),
                )
                .await
                .map_err(sql)?;
            }
            EnrollmentChange::PutInstallation(row) => {
                put_installation(scope, row, &at).await?;
            }
            EnrollmentChange::PutAllowance(allowance) => {
                put_allowance(scope, allowance, &at).await?;
            }
            EnrollmentChange::RekeyInstallation { .. } => {
                // The switch conversion keys every machine by its peer id, so the
                // facts above never report an unconverted former installation.
                return Err(StoreError::Aborted {
                    reason: "a re-key is never needed on a converted store".into(),
                });
            }
            EnrollmentChange::LinkFormerService {
                former,
                installation,
            } => {
                queries::link_former_service(
                    scope,
                    former.as_service_id().as_str().to_owned(),
                    at.clone(),
                    installation.as_str().to_owned(),
                )
                .await
                .map_err(sql)?;
                // Set once: the update only fills an empty former id.
                let linked = queries::read_installation(scope, installation.as_str().to_owned())
                    .await
                    .map_err(sql)?
                    .map(decode_installation)
                    .transpose()?
                    .and_then(|row| row.former_service_id);
                if linked.as_ref() != Some(former) {
                    return Err(corrupt(
                        "machine_installations.former_service_id",
                        "a former service id is set once and never replaced",
                    ));
                }
            }
            EnrollmentChange::Record(record) => change_records::insert(scope, record).await?,
            // Kept only by a store with a request fingerprint, pending the owner;
            // this store refuses a repeated request instead (`read_facts`).
            EnrollmentChange::RecordApprovalRequest(_) => {}
        }
    }
    Ok(())
}

/// The columns a transition must not change.
fn same_identity(stored: &StoredEnrollment, next: &StoredEnrollment) -> bool {
    stored.installation == next.installation
        && stored.request_id == next.request_id
        && stored.approved_by == next.approved_by
        && stored.base_generation == next.base_generation
        && stored.approved_at_ms == next.approved_at_ms
        && stored.code_expires_at == next.code_expires_at
}

async fn put_installation<S>(
    scope: &mut S,
    row: &InstallationRow,
    at: &str,
) -> Result<(), StoreError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let id = row.installation.as_str().to_owned();
    let generation = generation_text(
        row.generation,
        "machine_installations.enrollment_generation",
    )?;
    let label = row.label.as_str().to_owned();
    if queries::read_installation(scope, id.clone())
        .await
        .map_err(sql)?
        .is_some()
    {
        queries::update_installation(scope, label, generation, at.to_owned(), id)
            .await
            .map_err(sql)
    } else {
        queries::insert_installation(scope, id, label, generation, at.to_owned(), at.to_owned())
            .await
            .map_err(sql)
    }
}

async fn put_allowance<S>(
    scope: &mut S,
    allowance: &AllowanceRow,
    at: &str,
) -> Result<(), StoreError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let id = allowance.id.to_string();
    let state = allowance_state_text(&allowance.state).to_owned();
    let existing = queries::read_allowances(scope, allowance.installation.as_str().to_owned())
        .await
        .map_err(sql)?
        .into_iter()
        .map(decode_allowance)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .any(|row| row.id == allowance.id);
    if existing {
        queries::update_allowance(scope, state, at.to_owned(), id)
            .await
            .map_err(sql)
    } else {
        queries::insert_allowance(
            scope,
            id,
            allowance.installation.as_str().to_owned(),
            allowance.workspace.as_str().to_owned(),
            state,
            at.to_owned(),
            at.to_owned(),
        )
        .await
        .map_err(sql)
    }
}

#[cfg(test)]
#[path = "enrollment_sql_store_test.rs"]
mod tests;

#[cfg(test)]
#[path = "sql_constraint_test.rs"]
mod constraint_tests;

#[cfg(test)]
#[path = "sql_conversion_test.rs"]
mod conversion_tests;
