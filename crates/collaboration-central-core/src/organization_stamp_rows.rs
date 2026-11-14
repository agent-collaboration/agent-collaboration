//! The org-scoped central's single verified provisioning stamp.
use crate::organization_authority::{StoredOrganizationStamp, StoredStampRead};
use crate::{ObjectHost, workspace_decode_error::corrupt, workspace_row_codec::string};
use central_storage::{GuardedStatement, RowExpectation, SqlValue, Statement, StoreFailure};
use fleet_identity_protocol::OrganizationId;

pub(crate) async fn read(host: &impl ObjectHost) -> Result<StoredStampRead, StoreFailure> {
    let rows = host
        .read(Statement {
            sql: "SELECT id,organization_statement FROM organization_identity ORDER BY id LIMIT 2",
            args: vec![],
        })
        .await?;
    if rows.rows.len() > 1 {
        return Err(corrupt("organization_identity.id"));
    }
    let Some(row) = rows.rows.first() else {
        return Ok(StoredStampRead::Unprovisioned);
    };
    let organization = OrganizationId::try_from(string(&rows, row, "id")?.to_owned())
        .map_err(|_| corrupt("organization_identity.id"))?;
    let statement = string(&rows, row, "organization_statement")?.to_owned();
    if statement.is_empty() {
        return Err(corrupt("organization_identity.organization_statement"));
    }
    Ok(StoredStampRead::Stamped(StoredOrganizationStamp::new(
        organization,
        statement,
    )))
}
fn integer(value: i64) -> Result<SqlValue, StoreFailure> {
    value
        .try_into()
        .map(SqlValue::Integer)
        .map_err(|_| corrupt("organization_identity.updated_at"))
}
pub(crate) fn insert(
    organization: &OrganizationId,
    statement: &str,
    now_ms: i64,
    issued_seconds: i64,
    expires_seconds: i64,
) -> Result<GuardedStatement, StoreFailure> {
    Ok(GuardedStatement {
        statement: Statement {
            sql: "INSERT INTO organization_identity(id,organization_statement,created_at,updated_at) SELECT ?,?,?,? WHERE NOT EXISTS (SELECT 1 FROM organization_identity) AND ?>=? AND ?<?",
            args: vec![
                SqlValue::Text(organization.as_str().to_owned()),
                SqlValue::Text(statement.to_owned()),
                integer(now_ms)?,
                integer(now_ms)?,
                integer(now_ms.div_euclid(1000))?,
                integer(issued_seconds)?,
                integer(now_ms.div_euclid(1000))?,
                integer(expires_seconds)?,
            ],
        },
        expect: RowExpectation::Exactly(1),
    })
}
pub(crate) fn replace(
    stored: &StoredOrganizationStamp,
    statement: &str,
    now_ms: i64,
    issued_seconds: i64,
    expires_seconds: i64,
) -> Result<GuardedStatement, StoreFailure> {
    Ok(GuardedStatement {
        statement: Statement {
            sql: "UPDATE organization_identity SET organization_statement=?,updated_at=? WHERE id=? AND organization_statement=? AND ?>=? AND ?<?",
            args: vec![
                SqlValue::Text(statement.to_owned()),
                integer(now_ms)?,
                SqlValue::Text(stored.organization_id().as_str().to_owned()),
                SqlValue::Text(stored.statement().to_owned()),
                integer(now_ms.div_euclid(1000))?,
                integer(issued_seconds)?,
                integer(now_ms.div_euclid(1000))?,
                integer(expires_seconds)?,
            ],
        },
        expect: RowExpectation::Exactly(1),
    })
}
