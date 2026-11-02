//! Reports commit with their subject and remain owed until central acknowledges that exact sequence.
use crate::StorageError;
use collaboration_protocol::{
    AcceptedWorkReport, FiringId, FiringOutcome, FiringReport, OperationId, WorkItemReport,
    WorkReportSequence, WorkReportSubject,
};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, SqliteConnection};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CentralReportId(OperationId);
impl CentralReportId {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl TryFrom<String> for CentralReportId {
    type Error = StorageError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(Self(
            value.try_into().map_err(|_| StorageError::InvalidRecord)?,
        ))
    }
}
impl From<CentralReportId> for String {
    fn from(value: CentralReportId) -> Self {
        value.0.as_str().to_owned()
    }
}

#[derive(Clone, Debug)]
pub struct QueuedCentralReport {
    pub report_id: CentralReportId,
    pub report: WorkItemReport,
    pub queued_at_ms: i64,
}
#[derive(Debug)]
pub enum CentralReportBatch {
    Empty,
    Waiting { due_at_ms: i64 },
    Ready(Vec<QueuedCentralReport>),
}
struct CentralReportRow {
    report_id: String,
    subject_kind: String,
    subject_id: String,
    report_seq: i64,
    report_json: String,
    queued_at_ms: i64,
}
impl CentralReportRow {
    fn decode(self) -> Result<QueuedCentralReport, StorageError> {
        let report: WorkItemReport =
            serde_json::from_str(&self.report_json).map_err(|_| StorageError::InvalidRecord)?;
        let (kind, id, seq) = report_identity(&report);
        if kind != self.subject_kind
            || id != self.subject_id
            || sequence_value(seq)? != self.report_seq
            || self.queued_at_ms < 0
        {
            return Err(StorageError::InvalidRecord);
        }
        Ok(QueuedCentralReport {
            report_id: self.report_id.try_into()?,
            report,
            queued_at_ms: self.queued_at_ms,
        })
    }
}
/// The oldest durable row is the deadline; sparse queues need no second write or timer record.
pub(crate) async fn central_report_batch(
    connection: &mut SqliteConnection,
    now_ms: i64,
) -> Result<CentralReportBatch, StorageError> {
    if now_ms < 0 {
        return Err(StorageError::InvalidRecord);
    }
    let rows = sqlx::query_as!(CentralReportRow,
            "SELECT report_id,subject_kind,subject_id,report_seq,report_json,queued_at_ms FROM central_report_outbox ORDER BY queued_at_ms,report_id LIMIT 100")
            .fetch_all(&mut *connection).await?;
    let reports = rows
        .into_iter()
        .map(CentralReportRow::decode)
        .collect::<Result<Vec<_>, _>>()?;
    let Some(first) = reports.first() else {
        return Ok(CentralReportBatch::Empty);
    };
    let due_at_ms = first
        .queued_at_ms
        .checked_add(i64::from(
            collaboration_protocol::limits::REPORT_FLUSH_WINDOW,
        ))
        .ok_or(StorageError::InvalidRecord)?;
    if reports.len() < collaboration_protocol::limits::REPORT_FLUSH_MAX && now_ms < due_at_ms {
        Ok(CentralReportBatch::Waiting { due_at_ms })
    } else {
        Ok(CentralReportBatch::Ready(reports))
    }
}
pub(crate) async fn acknowledge_central_reports(
    connection: &mut SqliteConnection,
    accepted: &[AcceptedWorkReport],
) -> Result<(), StorageError> {
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    for acknowledgement in accepted {
        let (kind, id) = match &acknowledgement.subject {
            WorkReportSubject::Firing { firing_id } => ("firing", firing_id.as_str()),
            WorkReportSubject::SessionCreate { work_item_id } => {
                ("workItem", work_item_id.as_str())
            }
        };
        let sequence = sequence_value(acknowledgement.seq)?;
        sqlx::query!("DELETE FROM central_report_outbox WHERE subject_kind=? AND subject_id=? AND report_seq=?", kind, id, sequence)
                .execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub(crate) fn report_identity(report: &WorkItemReport) -> (&'static str, &str, WorkReportSequence) {
    match report {
        WorkItemReport::Firing(report) => ("firing", report.firing_id.as_str(), report.seq),
        WorkItemReport::SessionCreate {
            work_item_id, seq, ..
        } => ("workItem", work_item_id.as_str(), *seq),
    }
}
fn sequence_value(sequence: WorkReportSequence) -> Result<i64, StorageError> {
    i64::try_from(u64::from(sequence)).map_err(|_| StorageError::InvalidRecord)
}
pub(crate) async fn insert_report(
    connection: &mut SqliteConnection,
    report: &WorkItemReport,
    now_ms: i64,
) -> Result<(), StorageError> {
    if now_ms < 0 {
        return Err(StorageError::InvalidRecord);
    }
    let report_id = OperationId::generate();
    let (kind, subject_id, seq) = report_identity(report);
    let seq = sequence_value(seq)?;
    let report_json = serde_json::to_string(report).map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query!("INSERT INTO central_report_outbox(report_id,subject_kind,subject_id,report_seq,report_json,queued_at_ms) VALUES (?,?,?,?,?,?)",
        report_id.as_str(),kind,subject_id,seq,report_json,now_ms).execute(connection).await?;
    Ok(())
}

pub(crate) async fn queue_firing_outcome(
    connection: &mut SqliteConnection,
    firing_id: &FiringId,
    outcome: FiringOutcome,
    now_ms: i64,
) -> Result<(), StorageError> {
    let row = sqlx::query!(
        "SELECT state,report_seq FROM firing_receipts WHERE firing_id=?",
        firing_id.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let previous: FiringOutcome =
        serde_json::from_str(&row.state).map_err(|_| StorageError::InvalidRecord)?;
    let previous_sequence: WorkReportSequence = u64::try_from(row.report_seq)
        .map_err(|_| StorageError::InvalidRecord)?
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    let state = serde_json::to_string(&outcome).map_err(|_| StorageError::InvalidRecord)?;
    if previous.is_terminal() || row.state == state {
        return Ok(());
    }
    let next_sequence = u64::from(previous_sequence)
        .checked_add(1)
        .ok_or(StorageError::InvalidRecord)?;
    let seq: WorkReportSequence = next_sequence
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    let sequence = sequence_value(seq)?;
    sqlx::query!(
        "UPDATE firing_receipts SET state=?,report_seq=? WHERE firing_id=?",
        state,
        sequence,
        firing_id.as_str()
    )
    .execute(&mut *connection)
    .await?;
    insert_report(
        connection,
        &WorkItemReport::Firing(FiringReport {
            firing_id: firing_id.clone(),
            seq,
            outcome,
        }),
        now_ms,
    )
    .await
}
