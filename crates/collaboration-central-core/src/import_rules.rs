use crate::copy_storage::*;
use crate::row_apply::{primary_columns, quoted_identifier, schema_fingerprint, table_columns};
use crate::*;
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredImport {
    pub digest: ImportDigest,
    pub import_epoch: Epoch,
    pub header: ImportSnapshotHeader,
}
impl StoredImport {
    fn unheld(&self) -> UnheldProject {
        UnheldProject {
            project: self.header.project.clone(),
            import_epoch: self.import_epoch,
            imported_head: self.header.head,
            schema_version: self.header.schema.version,
        }
    }
}
fn import_epoch() -> Result<Epoch, CentralError> {
    Epoch::new(1).map_err(|e| CentralError::InvalidInput {
        message: e.to_string(),
    })
}
fn result(answer: ProjectImportAnswer, unheld: Option<UnheldProject>) -> ImportSeedResult {
    ImportSeedResult { answer, unheld }
}
fn invalid(field: &str) -> ProjectImportRefusal {
    ProjectImportRefusal::InvalidPage {
        field: field.into(),
    }
}
fn rejected(id: &ObjectCallId, reason: ProjectImportRefusal) -> CentralError {
    CentralError::ImportRejected {
        call_id: id.clone(),
        reason,
    }
}
fn staged(tx: &mut dyn SqlTx) -> Result<Vec<ProjectImportPage>, CentralError> {
    tx.query(query_statement(
        "SELECT record FROM _copy_import_pages ORDER BY page_index",
        Vec::new(),
        ["record"],
    ))?
    .iter()
    .map(|row| decode_record(row.text("record")?, "import page"))
    .collect()
}
fn same_page(left: &ProjectImportPage, right: &ProjectImportPage) -> bool {
    left.digest == right.digest
        && left.index == right.index
        && ImportSnapshotHeader::from(&left.page) == ImportSnapshotHeader::from(&right.page)
        && left.page.table == right.page.table
        && left.page.rows == right.page.rows
        && left.page.next == right.page.next
        && left.final_summary == right.final_summary
}
pub(crate) fn seed(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    project: &ProjectId,
    upload: &ProjectImportPage,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<(ImportSeedResult, Vec<CopyEffect>), CentralError> {
    if let Some(state) = load_copy(tx)? {
        let imported: Option<StoredImport> = crate::registry_storage::read_record(
            tx,
            "SELECT record FROM _copy_import_metadata WHERE singleton=?",
            "1",
        )?;
        return Ok(match imported {
            Some(record) if record.digest == upload.digest && state.project == *project => (
                result(ProjectImportAnswer::AlreadyImported, Some(record.unheld())),
                Vec::new(),
            ),
            existing => (
                result(
                    ProjectImportAnswer::ImportConflict {
                        existing: existing.map_or(ExistingImport::NotImported, |record| {
                            ExistingImport::Imported(record.digest)
                        }),
                        offered: upload.digest.clone(),
                    },
                    None,
                ),
                Vec::new(),
            ),
        });
    }
    let mut pages = staged(tx)?;
    if pages
        .first()
        .is_some_and(|page| page.digest != upload.digest)
    {
        tx.execute(statement("DELETE FROM _copy_import_pages", Vec::new()))?;
        pages.clear();
    }
    let expected = u64::try_from(pages.len()).map_err(|_| CentralError::ReplyContract)?;
    if upload.index < expected {
        let repeated = pages.iter().find(|page| page.index == upload.index);
        if repeated.is_some_and(|page| same_page(page, upload)) {
            return Ok((
                result(
                    ProjectImportAnswer::Staged {
                        digest: upload.digest.clone(),
                        index: upload.index,
                    },
                    None,
                ),
                Vec::new(),
            ));
        }
        return Ok((
            result(
                ProjectImportAnswer::Refused(ProjectImportRefusal::UnexpectedIndex { expected }),
                None,
            ),
            Vec::new(),
        ));
    }
    if upload.index != expected {
        return Ok((
            result(
                ProjectImportAnswer::Refused(ProjectImportRefusal::UnexpectedIndex { expected }),
                None,
            ),
            Vec::new(),
        ));
    }
    let header = ImportSnapshotHeader::from(&upload.page);
    if header.project != *project
        || header.head.epoch != import_epoch()?
        || pages
            .first()
            .is_some_and(|page| ImportSnapshotHeader::from(&page.page) != header)
    {
        return Err(rejected(id, invalid("header")));
    }
    let final_page = upload.page.next.is_none();
    if final_page != upload.final_summary.is_some() {
        return Err(rejected(id, invalid("final_summary")));
    }
    tx.execute(statement(
        "INSERT INTO _copy_import_pages(page_index,record) VALUES(?,?)",
        vec![
            SqlValue::Integer(
                i64::try_from(upload.index).map_err(|_| rejected(id, invalid("index")))?,
            ),
            text(encode_record(upload)?),
        ],
    ))?;
    if !final_page {
        return Ok((
            result(
                ProjectImportAnswer::Staged {
                    digest: upload.digest.clone(),
                    index: upload.index,
                },
                None,
            ),
            Vec::new(),
        ));
    }
    pages.push(upload.clone());
    let summary = upload
        .final_summary
        .as_ref()
        .ok_or_else(|| rejected(id, invalid("final_summary")))?;
    publish_snapshot(tx, id, &header, &pages, summary, &upload.digest)?;
    let record = StoredImport {
        digest: upload.digest.clone(),
        import_epoch: import_epoch()?,
        header: header.clone(),
    };
    tx.execute(statement(
        "INSERT INTO _copy_import_metadata(singleton,record) VALUES(1,?)",
        vec![text(encode_record(&record)?)],
    ))?;
    save_copy(
        tx,
        &CopyStatus {
            project: project.clone(),
            fence: import_epoch()?,
            holder: None,
            start: ReplicationPosition::new(import_epoch()?, 0.into()),
            acked: header.head,
            schema: header.schema.clone(),
            archived: false,
            imported: Some(ImportedCopy {
                digest: upload.digest.clone(),
                import_epoch: import_epoch()?,
            }),
        },
    )?;
    tx.execute(statement("DELETE FROM _copy_import_pages", Vec::new()))?;
    let effects = crate::directory_producer::summary(tx, project, summary, header.head, now, ids)?;
    Ok((
        result(
            ProjectImportAnswer::Imported { head: header.head },
            Some(record.unheld()),
        ),
        effects,
    ))
}
fn tables(tx: &mut dyn SqlTx) -> Result<Vec<String>, CentralError> {
    Ok(tx
        .query(query_statement(
            "SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name",
            Vec::new(),
            ["name"],
        ))?
        .iter()
        .map(|row| row.text("name").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|name| !reserved_namespace(name))
        .collect())
}
fn publish_snapshot(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    header: &ImportSnapshotHeader,
    pages: &[ProjectImportPage],
    summary: &ProjectSummary,
    offered: &ImportDigest,
) -> Result<(), CentralError> {
    tx.execute(statement("PRAGMA defer_foreign_keys=ON", Vec::new()))?;
    for (index, sql) in header.schema_steps.iter().enumerate() {
        admit_schema_statement(sql).map_err(|error| {
            rejected(
                id,
                ProjectImportRefusal::SchemaUnsupported { form: error.form },
            )
        })?;
        tx.execute(statement(sql, Vec::new()))?;
        tx.execute(statement("INSERT INTO _copy_schema_steps(migration_version,statement_index,statement) VALUES(?,?,?)",vec![unsigned(header.schema.version),SqlValue::Integer(i64::try_from(index).map_err(|_|rejected(id,invalid("schema_steps")))?),text(sql)]))?;
    }
    if schema_fingerprint(tx)? != header.schema.fingerprint {
        return Err(rejected(id, ProjectImportRefusal::SchemaMismatch));
    }
    let names = tables(tx)?;
    // Schema data migrations may create rows. The snapshot is the complete state.
    for table in &names {
        tx.execute(statement(
            &format!("DELETE FROM {}", quoted_identifier(table)),
            Vec::new(),
        ))?;
    }
    for page in pages {
        if page.page.table.is_empty() && page.page.rows.is_empty() && names.is_empty() {
            continue;
        }
        let columns = table_columns(tx, &page.page.table)?;
        let column_names = columns
            .iter()
            .map(|column| quoted_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(",");
        let placeholders = columns.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        for row in &page.page.rows {
            if row.len() != columns.len() {
                return Err(rejected(id, invalid("row width")));
            }
            for key in primary_columns(&columns) {
                let key_index = columns
                    .iter()
                    .position(|column| column.name == key.name)
                    .ok_or(CentralError::ReplyContract)?;
                if matches!(row.get(key_index), Some(SqlValue::Null) | None) {
                    return Err(rejected(id, invalid("primary key")));
                }
            }
            tx.execute(statement(
                &format!(
                    "INSERT INTO {}({column_names}) VALUES({placeholders})",
                    quoted_identifier(&page.page.table)
                ),
                row.clone(),
            ))?;
        }
    }
    if !tx
        .query(BoundStatement::existence(
            "PRAGMA foreign_key_check",
            Vec::new(),
        ))?
        .is_empty()
    {
        return Err(rejected(id, invalid("foreign key")));
    }
    let mut digest = ImportDigestBuilder::new(header).map_err(|error| CentralError::Encoding {
        message: error.to_string(),
    })?;
    for table in names {
        let columns = table_columns(tx, &table)?;
        let projections = columns
            .iter()
            .map(|column| quoted_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(",");
        let keys = primary_columns(&columns)
            .iter()
            .map(|column| quoted_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(",");
        for row in tx.query(query_statement(
            format!(
                "SELECT {projections} FROM {} ORDER BY {keys}",
                quoted_identifier(&table)
            )
            .as_str(),
            Vec::new(),
            columns.iter().map(|column| column.name.as_str()),
        ))? {
            let values = columns
                .iter()
                .map(|column| row.value(&column.name).cloned())
                .collect::<Result<Vec<_>, _>>()?;
            digest
                .append_row(&table, &values)
                .map_err(|error| CentralError::Encoding {
                    message: error.to_string(),
                })?;
        }
    }
    let computed = digest
        .finish(summary)
        .map_err(|error| CentralError::Encoding {
            message: error.to_string(),
        })?;
    if computed != *offered {
        return Err(rejected(
            id,
            ProjectImportRefusal::DigestMismatch {
                computed,
                offered: offered.clone(),
            },
        ));
    }
    Ok(())
}
