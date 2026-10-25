//! Exact structural validation for the current automation migration target.
//!
//! Future migrations must update these target specifications with their schema change.
use crate::StorageError;
use sqlx::{Row, Sqlite, Transaction};

use crate::schema_definition_catalogue::{
    CURRENT_TARGET_DEFINITION_SOURCE, INDEX_SPECS, TABLE_SPECS, TableSpec,
};

pub(crate) async fn has_domain_objects(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<bool, StorageError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations'",
    )
    .fetch_one(&mut **transaction)
    .await?;
    Ok(count != 0)
}

pub(crate) async fn validate_target_schema(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    validate_schema(transaction, false).await
}

pub(crate) async fn validate_legacy_schema(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    validate_schema(transaction, true).await
}

async fn validate_schema(
    transaction: &mut Transaction<'_, Sqlite>,
    legacy: bool,
) -> Result<(), StorageError> {
    validate_object_inventory(transaction, legacy).await?;
    for table in TABLE_SPECS.iter().filter(|table| {
        !legacy || !matches!(table.name, "owed_push_operations" | "remote_wake_requests")
    }) {
        validate_columns(transaction, table, legacy).await?;
        validate_foreign_keys(transaction, table).await?;
    }
    validate_indexes(transaction, legacy).await?;
    validate_known_definitions(transaction, legacy).await?;
    Ok(())
}

async fn validate_object_inventory(
    transaction: &mut Transaction<'_, Sqlite>,
    legacy: bool,
) -> Result<(), StorageError> {
    let rows = sqlx::query(
        "SELECT type,name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY type,name",
    )
    .fetch_all(&mut **transaction)
    .await?;
    let mut actual_tables = Vec::new();
    let mut actual_named_indexes = Vec::new();
    for row in rows {
        let object_type: String = row.get("type");
        let name: String = row.get("name");
        match object_type.as_str() {
            "table" => actual_tables.push(name),
            "index" => actual_named_indexes.push(name),
            _ => return Err(StorageError::InvalidSchema),
        }
    }
    let expected_tables: Vec<&str> = TABLE_SPECS
        .iter()
        .filter(|table| {
            !legacy || !matches!(table.name, "owed_push_operations" | "remote_wake_requests")
        })
        .map(|table| table.name)
        .collect();
    let expected_indexes = if legacy {
        vec![
            "delivery_eligibility",
            "event_cleanup",
            "event_history",
            "run_admission_lookup",
            "run_history",
        ]
    } else {
        vec![
            "delivery_eligibility",
            "event_cleanup",
            "event_history",
            "owed_push_operations_order",
            "run_admission_lookup",
            "run_history",
            "wakeup_occupying_target",
        ]
    };
    if actual_tables != expected_tables || actual_named_indexes != expected_indexes {
        return Err(StorageError::InvalidSchema);
    }
    Ok(())
}

async fn validate_columns(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &TableSpec,
    legacy: bool,
) -> Result<(), StorageError> {
    let rows = sqlx::query(
        "SELECT name,type,\"notnull\",coalesce(dflt_value,'<NULL>') AS default_value,pk FROM pragma_table_info(?1) ORDER BY cid",
    )
    .bind(table.name)
    .fetch_all(&mut **transaction)
    .await?;
    let actual = rows
        .into_iter()
        .map(|row| {
            format!(
                "{},{},{},{},{}",
                row.get::<String, _>("name"),
                row.get::<String, _>("type"),
                row.get::<i64, _>("notnull"),
                row.get::<String, _>("default_value"),
                row.get::<i64, _>("pk")
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    let expected = if !legacy && table.name == "mailbox_deliveries" {
        table
            .columns
            .replace("accepted_receipt_json", "outcome_receipt_json")
    } else {
        table.columns.to_owned()
    };
    if actual != expected {
        return Err(StorageError::InvalidSchema);
    }
    Ok(())
}

async fn validate_foreign_keys(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &TableSpec,
) -> Result<(), StorageError> {
    let rows = sqlx::query(
        "SELECT id,seq,\"table\",\"from\",\"to\",on_update,on_delete,match FROM pragma_foreign_key_list(?1) ORDER BY id,seq",
    )
    .bind(table.name)
    .fetch_all(&mut **transaction)
    .await?;
    let actual = rows
        .into_iter()
        .map(|row| {
            format!(
                "{},{},{},{},{},{},{},{}",
                row.get::<i64, _>("id"),
                row.get::<i64, _>("seq"),
                row.get::<String, _>("table"),
                row.get::<String, _>("from"),
                row.get::<String, _>("to"),
                row.get::<String, _>("on_update"),
                row.get::<String, _>("on_delete"),
                row.get::<String, _>("match")
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    if actual != table.foreign_keys {
        return Err(StorageError::InvalidSchema);
    }
    Ok(())
}

async fn validate_indexes(
    transaction: &mut Transaction<'_, Sqlite>,
    legacy: bool,
) -> Result<(), StorageError> {
    let mut actual = Vec::new();
    for table in TABLE_SPECS.iter().filter(|table| {
        !legacy || !matches!(table.name, "owed_push_operations" | "remote_wake_requests")
    }) {
        let indexes = sqlx::query(
            "SELECT name,\"unique\",origin,partial FROM pragma_index_list(?1) ORDER BY seq",
        )
        .bind(table.name)
        .fetch_all(&mut **transaction)
        .await?;
        for index in indexes {
            let name: String = index.get("name");
            let origin: String = index.get("origin");
            let columns = sqlx::query(
                "SELECT name,desc,coll,key FROM pragma_index_xinfo(?1) WHERE key=1 ORDER BY seqno",
            )
            .bind(&name)
            .fetch_all(&mut **transaction)
            .await?
            .into_iter()
            .map(|column| {
                format!(
                    "{}:{}:{}:{}",
                    column
                        .get::<Option<String>, _>("name")
                        .unwrap_or_else(|| "<EXPRESSION>".into()),
                    column.get::<i64, _>("desc"),
                    column.get::<String, _>("coll"),
                    column.get::<i64, _>("key")
                )
            })
            .collect::<Vec<_>>();
            actual.push(format!(
                "{},{},{},{},{},{}",
                table.name,
                if origin == "c" { name.as_str() } else { "_" },
                index.get::<i64, _>("unique"),
                origin,
                index.get::<i64, _>("partial"),
                columns.join(",")
            ));
        }
    }
    actual.sort();
    let mut expected = INDEX_SPECS
        .iter()
        .filter(|index| {
            !legacy
                || (!index.starts_with("remote_wake_requests,")
                    && !index.starts_with("owed_push_operations,")
                    && !index.starts_with("wakeup_definitions,wakeup_occupying_target,"))
        })
        .copied()
        .collect::<Vec<_>>();
    expected.sort();
    if actual != expected {
        return Err(StorageError::InvalidSchema);
    }
    Ok(())
}

async fn validate_known_definitions(
    transaction: &mut Transaction<'_, Sqlite>,
    legacy: bool,
) -> Result<(), StorageError> {
    for table in TABLE_SPECS.iter().filter(|table| {
        !legacy || !matches!(table.name, "owed_push_operations" | "remote_wake_requests")
    }) {
        let sql: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1")
                .bind(table.name)
                .fetch_one(&mut **transaction)
                .await?;
        if tokenize_schema_definition(&sql)?
            != expected_definition_tokens("table", table.name, legacy)?
        {
            return Err(StorageError::InvalidSchema);
        }
    }
    for index in INDEX_SPECS
        .iter()
        .filter(|index| {
            !legacy
                || (!index.starts_with("remote_wake_requests,")
                    && !index.starts_with("owed_push_operations,")
                    && !index.starts_with("wakeup_definitions,wakeup_occupying_target,"))
        })
        .filter_map(|index| {
            let mut fields = index.split(',');
            fields.next();
            let name = fields.next()?;
            (name != "_").then_some(name)
        })
    {
        let sql: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='index' AND name=?1")
                .bind(index)
                .fetch_one(&mut **transaction)
                .await?;
        if tokenize_schema_definition(&sql)? != expected_definition_tokens("index", index, legacy)?
        {
            return Err(StorageError::InvalidSchema);
        }
    }
    Ok(())
}

fn expected_definition_tokens(
    object_type: &str,
    object_name: &str,
    legacy: bool,
) -> Result<Vec<String>, StorageError> {
    let source = if legacy {
        CURRENT_TARGET_DEFINITION_SOURCE.to_owned()
    } else {
        CURRENT_TARGET_DEFINITION_SOURCE.replace("accepted_receipt_json", "outcome_receipt_json")
    };
    let tokens = tokenize_schema_definition(&source)?;
    let index_is_unique = if object_type == "index" {
        index_is_declared_unique(object_name)?
    } else {
        false
    };
    let definition = tokens
        .split(|token| token == ";")
        .find(|statement| {
            definition_prefix_matches(statement, object_type, object_name, index_is_unique)
        })
        .map(<[String]>::to_vec)
        .ok_or(StorageError::InvalidSchema)?;
    Ok(definition)
}

fn index_is_declared_unique(index_name: &str) -> Result<bool, StorageError> {
    INDEX_SPECS
        .iter()
        .find_map(|index| {
            let mut fields = index.split(',');
            fields.next()?;
            let name = fields.next()?;
            let unique = fields.next()?;
            (name == index_name).then_some(unique == "1")
        })
        .ok_or(StorageError::InvalidSchema)
}

fn definition_prefix_matches(
    statement: &[String],
    object_type: &str,
    object_name: &str,
    index_is_unique: bool,
) -> bool {
    let matches_prefix = |prefix: &[&str]| {
        statement
            .get(..prefix.len())
            .is_some_and(|tokens| tokens.iter().map(String::as_str).eq(prefix.iter().copied()))
    };
    match (object_type, index_is_unique) {
        ("index", true) => matches_prefix(&["create", "unique", "index", object_name]),
        ("index", false) => matches_prefix(&["create", "index", object_name]),
        ("table", _) => matches_prefix(&["create", "table", object_name]),
        _ => false,
    }
}

#[cfg(test)]
fn contains_token_sequence(tokens: &[String], expected: &[&str]) -> bool {
    tokens.windows(expected.len()).any(|window| {
        window
            .iter()
            .map(String::as_str)
            .eq(expected.iter().copied())
    })
}

fn tokenize_schema_definition(sql: &str) -> Result<Vec<String>, StorageError> {
    let mut tokens = Vec::new();
    let mut characters = sql.chars().peekable();
    while let Some(character) = characters.next() {
        if character.is_whitespace() {
            continue;
        }
        if character == '-' && characters.peek() == Some(&'-') {
            characters.next();
            for comment_character in characters.by_ref() {
                if comment_character == '\n' {
                    break;
                }
            }
            continue;
        }
        if character == '/' && characters.peek() == Some(&'*') {
            characters.next();
            let mut previous = '\0';
            loop {
                let Some(comment_character) = characters.next() else {
                    return Err(StorageError::InvalidSchema);
                };
                if previous == '*' && comment_character == '/' {
                    break;
                }
                previous = comment_character;
            }
            continue;
        }
        if character == '\'' {
            let mut literal = String::from("'");
            loop {
                let Some(literal_character) = characters.next() else {
                    return Err(StorageError::InvalidSchema);
                };
                literal.push(literal_character);
                if literal_character == '\'' {
                    if characters.peek() == Some(&'\'') {
                        let Some(escaped_quote) = characters.next() else {
                            return Err(StorageError::InvalidSchema);
                        };
                        literal.push(escaped_quote);
                    } else {
                        break;
                    }
                }
            }
            tokens.push(literal);
            continue;
        }
        if matches!(character, '"' | '`' | '[') {
            let terminator = if character == '[' { ']' } else { character };
            let mut identifier = String::new();
            loop {
                let Some(identifier_character) = characters.next() else {
                    return Err(StorageError::InvalidSchema);
                };
                if identifier_character == terminator {
                    if characters.peek() == Some(&terminator) && terminator != ']' {
                        let Some(escaped_terminator) = characters.next() else {
                            return Err(StorageError::InvalidSchema);
                        };
                        identifier.push(escaped_terminator);
                    } else {
                        break;
                    }
                } else {
                    identifier.push(identifier_character);
                }
            }
            tokens.push(identifier.to_ascii_lowercase());
            continue;
        }
        if character.is_ascii_alphanumeric() || character == '_' {
            let mut word = String::from(character);
            while let Some(word_character) =
                characters.next_if(|next| next.is_ascii_alphanumeric() || *next == '_')
            {
                word.push(word_character);
            }
            tokens.push(word.to_ascii_lowercase());
            continue;
        }
        tokens.push(character.to_string());
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_tokenizer_preserves_literal_contents_and_token_boundaries() {
        let tokens = tokenize_schema_definition(
            "CHECK (value = 'Case Sensitive' AND quoted = \"Value Name\") -- ignored",
        )
        .expect("known schema should tokenize");
        assert!(tokens.contains(&"'Case Sensitive'".to_owned()));
        assert!(contains_token_sequence(
            &tokens,
            &["check", "(", "value", "=", "'Case Sensitive'"]
        ));
        assert_ne!(
            tokens,
            tokenize_schema_definition("CHECK(value='casesensitive')").unwrap()
        );
        assert!(tokens.contains(&"value name".to_owned()));
    }

    #[test]
    fn non_unique_same_name_index_does_not_match_unique_index_spec() {
        let is_unique =
            index_is_declared_unique("wakeup_occupying_target").expect("declared index");
        assert!(is_unique);
        let non_unique_definition = tokenize_schema_definition(
            "CREATE INDEX wakeup_occupying_target ON wakeup_definitions(enabled)",
        )
        .expect("non-unique index definition should tokenize");
        assert!(!definition_prefix_matches(
            &non_unique_definition,
            "index",
            "wakeup_occupying_target",
            is_unique,
        ));
        let unique_definition = tokenize_schema_definition(
            "CREATE UNIQUE INDEX wakeup_occupying_target ON wakeup_definitions(enabled)",
        )
        .expect("unique index definition should tokenize");
        assert!(definition_prefix_matches(
            &unique_definition,
            "index",
            "wakeup_occupying_target",
            is_unique,
        ));
    }
}
