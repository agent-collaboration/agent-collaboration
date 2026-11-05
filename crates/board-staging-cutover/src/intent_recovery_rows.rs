//! Stream exact old rows; retain only the small workset of named recovery changes.
use crate::{NativeProjectSnapshot, PreparedProjectBundle};
use fleet_sync_protocol::{SnapshotManifest, SnapshotPage, SqlValue};
use message_board::IntentState;
use std::{cmp::Ordering, collections::BTreeMap};

pub(crate) const TABLES: [(&str, usize); 7] = [
    ("actors", 2),
    ("epoch_lines", 3),
    ("handoff_frontier", 2),
    ("mail_handoffs", 3),
    ("notify_intents", 12),
    ("project_feed", 12),
    ("request_decisions", 9),
];
pub(crate) type Row = Vec<SqlValue>;
pub(crate) type RecoveryResult<T> = Result<T, RecoveryError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum RecoveryError {
    #[error("recovery snapshot is invalid: {0}")]
    Invalid(&'static str),
    #[error("recovery changed an unexplained table or row")]
    Unexplained,
    #[error("cannot read the sealed or installed recovery snapshot")]
    Read,
}

pub(crate) struct RecoveryDelta {
    pub added: BTreeMap<String, Vec<Row>>,
    pub intents: Vec<(Row, Row)>,
    pub source_emitted: u64,
    pub existing_actor: Option<String>,
    pub source_handoff_head: i64,
    pub source_frontier: Option<Row>,
    pub changed_frontier: Option<Row>,
}

impl RecoveryDelta {
    pub fn added(&self, name: &str) -> &[Row] {
        self.added.get(name).map_or(&[], Vec::as_slice)
    }
    fn source(&mut self, table: &str, row: &Row, actor: &str) -> RecoveryResult<()> {
        if table == "handoff_frontier" && self.source_frontier.replace(row.clone()).is_some() {
            return Err(RecoveryError::Invalid("source frontier singleton"));
        }
        if table == "mail_handoffs" {
            let sequence = integer(row, 1)?;
            require(sequence > 0, "source handoff sequence")?;
            self.source_handoff_head = self.source_handoff_head.max(sequence);
        }
        if table == "notify_intents" && json::<IntentState>(row, 11)? == (IntentState::Emitted {}) {
            self.source_emitted = self
                .source_emitted
                .checked_add(1)
                .ok_or(RecoveryError::Invalid("intent count"))?;
        }
        if table == "actors"
            && text(row, 1)? == actor
            && self
                .existing_actor
                .replace(text(row, 0)?.to_owned())
                .is_some()
        {
            return Err(RecoveryError::Invalid("duplicate producer actor"));
        }
        Ok(())
    }
    fn added_row(&mut self, table: String, row: Row) -> RecoveryResult<()> {
        if table == "notify_intents" {
            return Err(RecoveryError::Unexplained);
        }
        self.added.entry(table).or_default().push(row);
        Ok(())
    }
}

pub(crate) async fn compare_recovery_rows(
    bundle: &PreparedProjectBundle,
    installed: &mut NativeProjectSnapshot<'_>,
    actor: &str,
) -> RecoveryResult<RecoveryDelta> {
    let before = &bundle.manifest().source.manifest;
    let after = installed.preflight().manifest.clone();
    if before.database != after.database
        || before.schema != after.schema
        || before.tables.len() != after.tables.len()
    {
        return Err(RecoveryError::Invalid("identity or schema"));
    }
    for (source, actual) in before.tables.iter().zip(&after.tables) {
        if source.table != actual.table {
            return Err(RecoveryError::Invalid("table order"));
        }
        if !TABLES
            .iter()
            .any(|(name, _)| *name == source.table.as_str())
            && source != actual
        {
            return Err(RecoveryError::Unexplained);
        }
    }
    installed.rewind_pages();
    let mut source = Rows::new(RowsSource::Prepared { bundle, page: 0 }, before)?;
    let mut target = Rows::new(RowsSource::Installed(installed), &after)?;
    let mut previous = source.next().await?;
    let mut current = target.next().await?;
    let mut delta = RecoveryDelta {
        added: BTreeMap::new(),
        intents: Vec::new(),
        source_emitted: 0,
        existing_actor: None,
        source_handoff_head: 0,
        source_frontier: None,
        changed_frontier: None,
    };
    loop {
        match (&previous, &current) {
            (None, None) => break,
            (Some(_), None) => return Err(RecoveryError::Unexplained),
            (None, Some(_)) => {
                let row = current.take().ok_or(RecoveryError::Invalid("target row"))?;
                delta.added_row(row.table, row.values)?;
                current = target.next().await?;
            }
            (Some(old), Some(new)) => match old.key.cmp(&new.key) {
                Ordering::Less => return Err(RecoveryError::Unexplained),
                Ordering::Greater => {
                    let row = current.take().ok_or(RecoveryError::Invalid("added row"))?;
                    delta.added_row(row.table, row.values)?;
                    current = target.next().await?;
                }
                Ordering::Equal => {
                    let old = previous
                        .take()
                        .ok_or(RecoveryError::Invalid("source row"))?;
                    let new = current
                        .take()
                        .ok_or(RecoveryError::Invalid("installed row"))?;
                    delta.source(&old.table, &old.values, actor)?;
                    if old.values != new.values {
                        match old.table.as_str() {
                            "notify_intents" => delta.intents.push((old.values, new.values)),
                            "handoff_frontier" => {
                                require(
                                    delta.changed_frontier.replace(new.values).is_none(),
                                    "frontier singleton",
                                )?;
                            }
                            _ => return Err(RecoveryError::Unexplained),
                        }
                    }
                    previous = source.next().await?;
                    current = target.next().await?;
                }
            },
        }
    }
    source.check_counts(before)?;
    target.check_counts(&after)?;
    Ok(delta)
}

enum RowsSource<'a, 'host> {
    Prepared {
        bundle: &'a PreparedProjectBundle,
        page: u32,
    },
    Installed(&'a mut NativeProjectSnapshot<'host>),
}
struct Entry {
    table: String,
    key: (usize, String),
    values: Row,
}
struct Rows<'a, 'host> {
    source: RowsSource<'a, 'host>,
    table: String,
    values: std::vec::IntoIter<Row>,
    order: BTreeMap<String, usize>,
    last: Option<(usize, String)>,
    counts: BTreeMap<String, u64>,
}
impl<'a, 'host> Rows<'a, 'host> {
    fn new(source: RowsSource<'a, 'host>, manifest: &SnapshotManifest) -> RecoveryResult<Self> {
        let order = manifest
            .tables
            .iter()
            .enumerate()
            .map(|(index, table)| (table.table.as_str().to_owned(), index))
            .collect();
        Ok(Self {
            source,
            table: String::new(),
            values: Vec::new().into_iter(),
            order,
            last: None,
            counts: TABLES
                .iter()
                .map(|(name, _)| ((*name).to_owned(), 0))
                .collect(),
        })
    }
    async fn next(&mut self) -> RecoveryResult<Option<Entry>> {
        loop {
            if let Some(values) = self.values.next() {
                let rank = *self
                    .order
                    .get(&self.table)
                    .ok_or(RecoveryError::Invalid("page table"))?;
                let key = (rank, text(&values, 0)?.to_owned());
                if self.last.as_ref().is_some_and(|last| last >= &key) {
                    return Err(RecoveryError::Invalid("primary key ordering"));
                }
                self.last = Some(key.clone());
                let count = self
                    .counts
                    .get_mut(&self.table)
                    .ok_or(RecoveryError::Invalid("row count"))?;
                *count = count
                    .checked_add(1)
                    .ok_or(RecoveryError::Invalid("row count overflow"))?;
                let width = TABLES
                    .iter()
                    .find(|(name, _)| *name == self.table)
                    .map(|(_, width)| *width)
                    .ok_or(RecoveryError::Invalid("row layout"))?;
                if values.len() != width {
                    return Err(RecoveryError::Invalid("current row layout"));
                }
                return Ok(Some(Entry {
                    table: self.table.clone(),
                    key,
                    values,
                }));
            }
            let page = match &mut self.source {
                RowsSource::Prepared { bundle, page } => {
                    if *page >= bundle.manifest().header.pages {
                        return Ok(None);
                    }
                    let contents = bundle.page(*page).map_err(|_| RecoveryError::Read)?;
                    *page = page
                        .checked_add(1)
                        .ok_or(RecoveryError::Invalid("page count"))?;
                    Some(contents)
                }
                RowsSource::Installed(snapshot) => snapshot
                    .next_page()
                    .await
                    .map_err(|_| RecoveryError::Read)?
                    .map(|page| page.contents),
            };
            let Some(SnapshotPage { table, rows, .. }) = page else {
                return Ok(None);
            };
            if TABLES.iter().any(|(name, _)| *name == table) {
                self.table = table;
                self.values = rows.into_iter();
            }
        }
    }
    fn check_counts(&self, manifest: &SnapshotManifest) -> RecoveryResult<()> {
        for (name, _) in TABLES {
            let expected = manifest
                .tables
                .iter()
                .find(|table| table.table.as_str() == name)
                .ok_or(RecoveryError::Invalid("manifest table"))?
                .row_count;
            if self.counts.get(name).copied() != Some(expected) {
                return Err(RecoveryError::Invalid("paged row count"));
            }
        }
        Ok(())
    }
}

pub(crate) fn cell(row: &[SqlValue], index: usize) -> RecoveryResult<&SqlValue> {
    row.get(index).ok_or(RecoveryError::Invalid("missing cell"))
}
pub(crate) fn text(row: &[SqlValue], index: usize) -> RecoveryResult<&str> {
    match cell(row, index)? {
        SqlValue::Text(value) => Ok(value),
        _ => Err(RecoveryError::Invalid("text cell")),
    }
}
pub(crate) fn integer(row: &[SqlValue], index: usize) -> RecoveryResult<i64> {
    match cell(row, index)? {
        SqlValue::Integer(value) => Ok(*value),
        _ => Err(RecoveryError::Invalid("integer cell")),
    }
}
pub(crate) fn json<T: serde::de::DeserializeOwned>(
    row: &[SqlValue],
    index: usize,
) -> RecoveryResult<T> {
    serde_json::from_str(text(row, index)?).map_err(|_| RecoveryError::Invalid("domain JSON"))
}
pub(crate) fn require(condition: bool, reason: &'static str) -> RecoveryResult<()> {
    if condition {
        Ok(())
    } else {
        Err(RecoveryError::Invalid(reason))
    }
}
