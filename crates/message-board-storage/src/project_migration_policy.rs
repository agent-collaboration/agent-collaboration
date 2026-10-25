//! The same policy runs at build time and in its permanent negative tests.
use turso_parser::{
    ast::{Cmd, ColumnConstraint, CreateTableBody, Stmt, TableConstraint},
    parser::Parser,
};

#[derive(Debug, thiserror::Error)]
pub enum MigrationPolicyError {
    #[error("migration SQL cannot be parsed: {0}")]
    Parse(String),
    #[error("project table {table} has no declared primary key")]
    MissingPrimaryKey { table: String },
    #[error("project migrations cannot create triggers")]
    Trigger,
    #[error("project migrations cannot create virtual tables")]
    VirtualTable,
}

pub fn validate_migration_sql(sql: &str) -> Result<(), MigrationPolicyError> {
    let mut parser = Parser::new(sql.as_bytes());
    while let Some(command) = parser
        .next_cmd()
        .map_err(|error| MigrationPolicyError::Parse(error.to_string()))?
    {
        let Cmd::Stmt(statement) = command else {
            continue;
        };
        match statement {
            Stmt::CreateTrigger { .. } => return Err(MigrationPolicyError::Trigger),
            Stmt::CreateVirtualTable(_) => return Err(MigrationPolicyError::VirtualTable),
            Stmt::CreateTable { tbl_name, body, .. } => {
                let has_key = match body {
                    CreateTableBody::ColumnsAndConstraints {
                        columns,
                        constraints,
                        ..
                    } => {
                        columns.iter().any(|column| {
                            column.constraints.iter().any(|constraint| {
                                matches!(constraint.constraint, ColumnConstraint::PrimaryKey { .. })
                            })
                        }) || constraints.iter().any(|constraint| {
                            matches!(constraint.constraint, TableConstraint::PrimaryKey { .. })
                        })
                    }
                    CreateTableBody::AsSelect(_) => false,
                };
                if !has_key {
                    return Err(MigrationPolicyError::MissingPrimaryKey {
                        table: tbl_name.name.as_str().to_owned(),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MigrationPolicyError, validate_migration_sql};

    #[test]
    fn inline_and_composite_keys_are_admitted() {
        assert!(validate_migration_sql("CREATE TABLE one(id INTEGER PRIMARY KEY); CREATE TABLE two(a TEXT,b TEXT,PRIMARY KEY(a,b)); CREATE INDEX lookup ON two(a) WHERE b IS NOT NULL;").is_ok());
    }
    #[test]
    fn unkeyed_tables_triggers_and_virtual_tables_are_refused() {
        assert!(matches!(
            validate_migration_sql("CREATE TABLE unkeyed(value TEXT)"),
            Err(MigrationPolicyError::MissingPrimaryKey { .. })
        ));
        assert!(matches!(
            validate_migration_sql("CREATE TABLE unkeyed AS SELECT 1 AS value"),
            Err(MigrationPolicyError::MissingPrimaryKey { .. })
        ));
        assert!(matches!(
            validate_migration_sql(
                "CREATE TRIGGER copied_twice AFTER INSERT ON one BEGIN INSERT INTO two(a) VALUES(new.id); END;"
            ),
            Err(MigrationPolicyError::Trigger)
        ));
        assert!(matches!(
            validate_migration_sql("CREATE VIRTUAL TABLE search USING fts5(text)"),
            Err(MigrationPolicyError::VirtualTable)
        ));
    }
}
