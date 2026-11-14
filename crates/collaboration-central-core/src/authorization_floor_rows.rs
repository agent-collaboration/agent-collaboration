//! Strict authorization-floor decoding and snapshot guards for modern central.
use crate::{
    ObjectHost,
    person_request::{
        FloorRevision, OrganizationFloor, OrganizationFloorState, PersonFloor, PersonFloorState,
    },
    workspace_decode_error::corrupt,
    workspace_row_codec::{column, string},
};
use central_storage::{
    GuardedStatement, ReadOutcome, SafeInteger, SqlValue, Statement, StoreFailure,
};

pub(crate) fn integer(value: i64, field: &str) -> Result<SqlValue, StoreFailure> {
    SafeInteger::new(value)
        .map(SqlValue::Integer)
        .map_err(|_| corrupt(field))
}

fn read_integer(rows: &ReadOutcome, row: &[SqlValue], field: &str) -> Result<i64, StoreFailure> {
    match column(rows, row, field)? {
        SqlValue::Integer(value) => Ok(value.get()),
        _ => Err(corrupt(field)),
    }
}

fn private_id(value: &str, field: &str) -> Result<(), StoreFailure> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| corrupt(field))?;
    if !runtime_primitives::uuid_v7(parsed) || parsed.to_string() != value {
        return Err(corrupt(field));
    }
    Ok(())
}

pub(crate) fn decode_organization_floor(
    rows: &ReadOutcome,
    organization: &str,
) -> Result<Option<OrganizationFloor>, StoreFailure> {
    if rows.rows.len() > 1 {
        return Err(corrupt("organization_authorization_floors.id"));
    }
    let Some(row) = rows.rows.first() else {
        return Ok(None);
    };
    if string(rows, row, "id")? != organization {
        return Err(corrupt("organization_authorization_floors.id"));
    }
    let policy_digest = string(rows, row, "permission_policy_digest")?.to_owned();
    if policy_digest.is_empty() {
        return Err(corrupt(
            "organization_authorization_floors.permission_policy_digest",
        ));
    }
    let reject_through_ms = read_integer(rows, row, "request_reject_through_ms")?;
    if reject_through_ms < 0 {
        return Err(corrupt(
            "organization_authorization_floors.request_reject_through_ms",
        ));
    }
    Ok(Some(OrganizationFloor {
        revision: FloorRevision::try_from(read_integer(rows, row, "minimum_revision")?)?,
        state: OrganizationFloorState::parse(string(rows, row, "state")?)?,
        policy_digest,
        reject_through_ms,
    }))
}

pub(crate) fn decode_person_floor(
    rows: &ReadOutcome,
    organization: &str,
    person: &str,
) -> Result<Option<PersonFloor>, StoreFailure> {
    if rows.rows.len() > 1 {
        return Err(corrupt(
            "person_organization_authorization_floors.person_id",
        ));
    }
    let Some(row) = rows.rows.first() else {
        return Ok(None);
    };
    let subject_id = string(rows, row, "id")?.to_owned();
    private_id(&subject_id, "person_organization_authorization_floors.id")?;
    if string(rows, row, "organization_id")? != organization
        || string(rows, row, "person_id")? != person
    {
        return Err(corrupt(
            "person_organization_authorization_floors.person_id",
        ));
    }
    Ok(Some(PersonFloor {
        subject_id,
        revision: FloorRevision::try_from(read_integer(rows, row, "minimum_revision")?)?,
        state: PersonFloorState::parse(string(rows, row, "state")?)?,
    }))
}

pub(crate) async fn read_organization_floor(
    host: &impl ObjectHost,
    organization: &str,
) -> Result<Option<OrganizationFloor>, StoreFailure> {
    let rows = host.read(Statement {
        sql: "SELECT id,minimum_revision,state,permission_policy_digest,request_reject_through_ms FROM organization_authorization_floors WHERE id=?",
        args: vec![SqlValue::Text(organization.to_owned())],
    }).await?;
    decode_organization_floor(&rows, organization)
}

pub(crate) async fn read_person_floor(
    host: &impl ObjectHost,
    organization: &str,
    person: &str,
) -> Result<Option<PersonFloor>, StoreFailure> {
    let rows = host.read(Statement {
        sql: "SELECT id,organization_id,person_id,minimum_revision,state FROM person_organization_authorization_floors WHERE organization_id=? AND person_id=?",
        args: vec![SqlValue::Text(organization.to_owned()), SqlValue::Text(person.to_owned())],
    }).await?;
    decode_person_floor(&rows, organization, person)
}

pub(crate) fn organization_state(state: OrganizationFloorState) -> &'static str {
    match state {
        OrganizationFloorState::Active => "active",
        OrganizationFloorState::Stopped => "stopped",
        OrganizationFloorState::Deleted => "deleted",
    }
}
pub(crate) fn person_state(state: PersonFloorState) -> &'static str {
    match state {
        PersonFloorState::Active => "active",
        PersonFloorState::Inactive => "inactive",
    }
}

/// A zero-row guard means the read snapshot changed; the caller retries admission.
pub(crate) fn organization_guard(
    organization: &str,
    floor: &OrganizationFloor,
) -> Result<GuardedStatement, StoreFailure> {
    Ok(crate::guarded_statements::exact(
        "UPDATE organization_authorization_floors SET id=id WHERE id=? AND minimum_revision=? AND state=? AND permission_policy_digest=? AND request_reject_through_ms=?",
        vec![
            SqlValue::Text(organization.to_owned()),
            integer(floor.revision.get(), "minimum_revision")?,
            SqlValue::Text(organization_state(floor.state).to_owned()),
            SqlValue::Text(floor.policy_digest.clone()),
            integer(floor.reject_through_ms, "request_reject_through_ms")?,
        ],
    ))
}
pub(crate) fn person_guard(
    organization: &str,
    person: &str,
    floor: &PersonFloor,
) -> Result<GuardedStatement, StoreFailure> {
    Ok(crate::guarded_statements::exact(
        "UPDATE person_organization_authorization_floors SET id=id WHERE id=? AND organization_id=? AND person_id=? AND minimum_revision=? AND state=?",
        vec![
            SqlValue::Text(floor.subject_id.clone()),
            SqlValue::Text(organization.to_owned()),
            SqlValue::Text(person.to_owned()),
            integer(floor.revision.get(), "minimum_revision")?,
            SqlValue::Text(person_state(floor.state).to_owned()),
        ],
    ))
}
