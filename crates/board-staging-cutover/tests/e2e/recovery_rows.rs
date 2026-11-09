//! Keep old snapshot bytes exact while checking each named recovery addition.
use super::offline_cli_fixture::TestResult;
use board_staging_cutover::{NativeProjectSnapshot, PreparedProjectBundle};
use fleet_protocol::{SnapshotManifest, SqlValue};
use std::collections::BTreeMap;

pub(super) type Row = Vec<SqlValue>;
pub(super) type Rows = BTreeMap<String, Vec<Row>>;
pub(super) const DELTAS: [(&str, u64); 6] = [
    ("actors", 1),
    ("epoch_lines", 1),
    ("mail_handoffs", 1),
    ("notify_intents", 0),
    ("project_feed", 2),
    ("request_decisions", 1),
];

pub(super) struct RecoveryRows {
    pub before: Rows,
    pub after: Rows,
}

impl RecoveryRows {
    pub async fn read(
        bundle: &PreparedProjectBundle,
        installed: &mut NativeProjectSnapshot<'_>,
    ) -> TestResult<Self> {
        let before_manifest = &bundle.manifest().header.manifest;
        let after_manifest = &installed.preflight().manifest;
        require(
            before_manifest.database == after_manifest.database
                && before_manifest.schema == after_manifest.schema,
            "installed identity or schema changed",
        )?;
        require(
            before_manifest.tables.len() == after_manifest.tables.len(),
            "installed table set changed",
        )?;
        for source in &before_manifest.tables {
            let actual = after_manifest
                .tables
                .iter()
                .find(|table| table.table == source.table)
                .ok_or("installed table missing")?;
            if source.table.as_str() == "handoff_frontier" {
                require(
                    source.row_count == 0 && actual.row_count <= 1,
                    "unexpected settlement frontier rows",
                )?;
            } else if let Some((_, delta)) = DELTAS
                .iter()
                .find(|(name, _)| *name == source.table.as_str())
            {
                let expected = source
                    .row_count
                    .checked_add(*delta)
                    .ok_or("row count overflow")?;
                require(
                    actual.row_count == expected,
                    &format!(
                        "{} expected {expected} recovery rows, got {}",
                        source.table.as_str(),
                        actual.row_count
                    ),
                )?;
            } else {
                require(
                    actual == source,
                    &format!("unexplained table change: {}", source.table.as_str()),
                )?;
            }
        }
        let mut before = Rows::new();
        for index in 0..bundle.manifest().header.pages {
            let page = bundle.page(index)?;
            append(&mut before, page.table, page.rows);
        }
        let mut after = Rows::new();
        while let Some(page) = installed.next_page().await? {
            append(&mut after, page.contents.table, page.contents.rows);
        }
        check_counts(&before, before_manifest)?;
        check_counts(&after, &installed.preflight().manifest)?;
        let frontier = after.get("handoff_frontier").map_or(&[][..], Vec::as_slice);
        let frontier_count = installed
            .preflight()
            .manifest
            .tables
            .iter()
            .find(|table| table.table.as_str() == "handoff_frontier")
            .ok_or("frontier manifest missing")?
            .row_count;
        require(
            u64::try_from(frontier.len())? == frontier_count,
            "frontier paging count differs",
        )?;
        if let [row] = frontier {
            require(
                row.len() == 2 && integer(row, 1)? == 1,
                "frontier did not cover exactly the recovered handoff",
            )?;
            let id = uuid::Uuid::parse_str(text(row, 0)?)?;
            require(
                runtime_primitives::uuid_v7(id),
                "frontier identifier is not UUIDv7",
            )?;
        }
        for (name, _) in DELTAS {
            if name != "notify_intents" {
                let prior = table(&before, name)?;
                let current = table(&after, name)?;
                for row in prior {
                    require(
                        contains(current, row)?,
                        &format!("old {name} row changed or disappeared"),
                    )?;
                }
            }
        }
        Ok(Self { before, after })
    }

    pub fn added(&self, name: &str) -> TestResult<Vec<&Row>> {
        let before = table(&self.before, name)?;
        table(&self.after, name)?
            .iter()
            .filter_map(|row| match contains(before, row) {
                Ok(false) => Some(Ok(row)),
                Ok(true) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }
}

fn append(rows: &mut Rows, name: String, values: Vec<Row>) {
    if name == "handoff_frontier" || DELTAS.iter().any(|(table, _)| *table == name) {
        rows.entry(name).or_default().extend(values);
    }
}
fn check_counts(rows: &Rows, manifest: &SnapshotManifest) -> TestResult {
    for (name, _) in DELTAS {
        let expected = manifest
            .tables
            .iter()
            .find(|table| table.table.as_str() == name)
            .ok_or("named table absent")?
            .row_count;
        require(
            u64::try_from(table(rows, name)?.len())? == expected,
            "snapshot paging omitted or repeated a row",
        )?;
    }
    Ok(())
}
pub(super) fn table<'a>(rows: &'a Rows, name: &str) -> TestResult<&'a [Row]> {
    rows.get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing table: {name}").into())
}
fn contains(rows: &[Row], expected: &Row) -> TestResult<bool> {
    let expected = serde_json::to_vec(expected)?;
    for row in rows {
        if serde_json::to_vec(row)? == expected {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn require(condition: bool, reason: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(reason.to_owned().into())
    }
}
pub(super) fn cell(row: &[SqlValue], index: usize) -> TestResult<&SqlValue> {
    row.get(index)
        .ok_or_else(|| "missing recovery row cell".into())
}
pub(super) fn text(row: &[SqlValue], index: usize) -> TestResult<&str> {
    match row.get(index) {
        Some(SqlValue::Text(value)) => Ok(value),
        _ => Err("invalid text cell in recovery row".into()),
    }
}
pub(super) fn integer(row: &[SqlValue], index: usize) -> TestResult<i64> {
    match row.get(index) {
        Some(SqlValue::Integer(value)) => Ok(*value),
        _ => Err("invalid integer cell in recovery row".into()),
    }
}
pub(super) fn json<T: serde::de::DeserializeOwned>(
    row: &[SqlValue],
    index: usize,
) -> TestResult<T> {
    serde_json::from_str(text(row, index)?)
        .map_err(|_| "invalid domain JSON in recovery row".into())
}
