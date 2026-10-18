//! First-wins rejected-tail evidence and its independently retrievable rows.
use crate::copy_storage::{encode_record, query_statement, statement, text, unsigned};
use crate::fleet_storage::corrupt;
use crate::{CentralError, RegistryTime, SqlRow, SqlTx};
use collaboration_replication_protocol::{
    Epoch, InstallationId, NoticeKind, ProjectId, RequestId, SessionAnswer, SessionRefusal,
    TailDigest, TailReportRow,
};

fn decode(row: &SqlRow) -> Result<TailReportRow, CentralError> {
    let project = ProjectId::new(row.text("project")?).map_err(|_| corrupt("tail project"))?;
    let epoch = Epoch::new(row.unsigned("epoch")?).map_err(|_| corrupt("tail epoch"))?;
    if project.as_str() != row.text("project")? || epoch.value().to_string() != row.text("epoch")? {
        return Err(corrupt("tail identity"));
    }
    let request_ids: Vec<RequestId> =
        serde_json::from_str(row.text("request_ids")?).map_err(|_| corrupt("tail request IDs"))?;
    let original: serde_json::Value =
        serde_json::from_str(row.text("request_ids")?).map_err(|_| corrupt("tail request IDs"))?;
    if serde_json::to_value(&request_ids).map_err(|_| corrupt("tail request IDs"))? != original {
        return Err(corrupt("tail request IDs"));
    }
    let digest =
        TailDigest::new(row.text("digest")?.to_owned()).map_err(|_| corrupt("tail digest"))?;
    Ok(TailReportRow {
        project,
        epoch,
        request_ids,
        digest,
    })
}

pub(crate) fn store(
    tx: &mut dyn SqlTx,
    report: &TailReportRow,
    current_holder: &InstallationId,
    now: RegistryTime,
) -> Result<SessionAnswer, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT project,epoch,request_ids,digest FROM tail_reports WHERE project=? AND epoch=?",
        vec![
            text(report.project.as_str()),
            unsigned(report.epoch.value()),
        ],
        ["project", "epoch", "request_ids", "digest"],
    ))?;
    if let Some(row) = rows.first() {
        let existing = decode(row)?;
        return Ok(if existing.digest == report.digest {
            SessionAnswer::TailStored
        } else {
            SessionAnswer::Refused(SessionRefusal::TailConflict {
                existing: existing.digest,
            })
        });
    }
    tx.execute(statement(
        "INSERT INTO tail_reports(project,epoch,request_ids,digest) VALUES(?,?,?,?)",
        vec![
            text(report.project.as_str()),
            unsigned(report.epoch.value()),
            text(encode_record(&report.request_ids)?),
            text(report.digest.as_str()),
        ],
    ))?;
    crate::fleet_notice_outbox::owe(
        tx,
        current_holder,
        &NoticeKind::RejectedTailReported {
            project: report.project.clone(),
            epoch: report.epoch,
        },
        now,
    )?;
    Ok(SessionAnswer::TailStored)
}

pub(crate) fn read(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<Vec<TailReportRow>, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT project,epoch,request_ids,digest FROM tail_reports WHERE project=?",
        vec![text(project.as_str())],
        ["project", "epoch", "request_ids", "digest"],
    ))?;
    let mut reports = rows.iter().map(decode).collect::<Result<Vec<_>, _>>()?;
    reports.sort_by_key(|report| report.epoch);
    Ok(reports)
}
