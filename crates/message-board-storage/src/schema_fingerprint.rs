//! The holder implements PD2's exact algorithm until the shared protocol lands in M4.

use message_board::SchemaFingerprint;
use sha2::{Digest, Sha256};
use sqlx_turso::TursoConnection;
use turso_parser::{
    ast::{
        Name,
        fmt::{BlankContext, ToTokens, TokenStream, WriteTokenStream},
    },
    parser::Parser,
    token::TokenType,
};

use crate::ProjectStorageError;

pub async fn schema_fingerprint(
    connection: &mut TursoConnection,
) -> Result<SchemaFingerprint, ProjectStorageError> {
    let rows = sqlx_turso::query!(
        r#"SELECT type AS "type!: String", name AS "name!: String", sql AS "sql?: String" FROM sqlite_schema ORDER BY type, name"#
    ).fetch_all(connection).await?;
    let entries = rows
        .into_iter()
        .map(|row| (row.r#type, row.name, row.sql.unwrap_or_default()))
        .collect::<Vec<_>>();
    fingerprint_entries(entries)
}

pub fn fingerprint_entries(
    mut entries: Vec<(String, String, String)>,
) -> Result<SchemaFingerprint, ProjectStorageError> {
    entries.retain(|(_, name, _)| {
        let name = name.to_ascii_lowercase();
        name != "_sqlx_migrations"
            && ![
                "sqlite_",
                "turso_",
                "__turso_internal",
                "_cf_",
                "__miniflare_",
                "_replication_",
                "_copy_",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    });
    for (_, _, sql) in &mut entries {
        *sql = canonical_schema_sql(sql)?;
    }
    entries.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    let bytes = serde_json::to_vec(&entries)?;
    SchemaFingerprint::try_from(format!("{:x}", Sha256::digest(bytes)))
        .map_err(|_| ProjectStorageError::InvalidSchema)
}

fn canonical_schema_sql(sql: &str) -> Result<String, ProjectStorageError> {
    let mut parser = Parser::new(sql.as_bytes());
    let command = parser
        .next_cmd()
        .map_err(|_| ProjectStorageError::InvalidSchema)?
        .ok_or(ProjectStorageError::InvalidSchema)?;
    if parser
        .next_cmd()
        .map_err(|_| ProjectStorageError::InvalidSchema)?
        .is_some()
    {
        return Err(ProjectStorageError::InvalidSchema);
    }
    let mut output = String::new();
    command
        .to_tokens(
            &mut CanonicalIdentifierStream {
                inner: WriteTokenStream::new(&mut output),
            },
            &BlankContext,
        )
        .map_err(|_| ProjectStorageError::InvalidSchema)?;
    Ok(output)
}

struct CanonicalIdentifierStream<'a> {
    inner: WriteTokenStream<'a, String>,
}
impl TokenStream for CanonicalIdentifierStream<'_> {
    type Error = std::fmt::Error;
    fn append(&mut self, kind: TokenType, value: Option<&str>) -> Result<(), Self::Error> {
        if kind == TokenType::TK_ID
            && let Some(value) = value
        {
            let canonical = Name::exact(Name::from_string(value).as_str().to_owned()).as_ident();
            return self.inner.append(kind, Some(&canonical));
        }
        self.inner.append(kind, value)
    }
}
