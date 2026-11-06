//! The committed Copy summary keeps its own position and domain time on the Fleet row.
use crate::{
    CopyDirectorySummary,
    fleet_column_codec::{canonical, decimal, optional_millis, optional_text, text},
    fleet_row_codec::corrupt,
    generated_fleet_record_reads::StoredDirectorySummary,
};
use board_sync_protocol::BoardAnnotation;
use durable_steps::StoreError;
use fleet_sync_protocol::{CommitSequence, DatabaseId, Epoch, ReplicationPosition, SyncTime};
use message_board::RecordTime;

pub(crate) fn decode(
    row: StoredDirectorySummary,
) -> Result<Option<CopyDirectorySummary>, StoreError> {
    let project: message_board::ProjectId = canonical(
        text(row.project_id, row.project_id_class, "copy.project")?,
        "copy.project",
    )?;
    let epoch = optional_text(row.copy_epoch, row.copy_epoch_class, "copy.epoch")?;
    let sequence = optional_text(row.copy_sequence, row.copy_sequence_class, "copy.sequence")?;
    let acked = optional_millis(row.copy_acked_at, row.copy_acked_at_class, "copy.acked_at")?;
    let basis = optional_text(
        row.copy_latest_time_basis,
        row.copy_latest_time_basis_class,
        "copy.latest_basis",
    )?;
    let at = optional_text(
        row.copy_latest_record_at,
        row.copy_latest_record_at_class,
        "copy.latest_at",
    )?;
    match (epoch, sequence, acked, basis, at) {
        (None, None, None, None, None) => Ok(None),
        (Some(epoch), Some(sequence), Some(acked), Some(basis), at) => {
            let latest_record_at = match (basis.as_str(), at) {
                ("legacy_unknown", None) => RecordTime::LegacyUnknown,
                ("router", Some(value)) => {
                    let time: chrono::DateTime<chrono::Utc> = value
                        .parse()
                        .map_err(|error| corrupt("copy.latest_at", error))?;
                    if time.to_rfc3339() != value
                        && time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true) != value
                    {
                        return Err(corrupt("copy.latest_at", "noncanonical domain timestamp"));
                    }
                    RecordTime::Router(time)
                }
                _ => {
                    return Err(corrupt(
                        "copy.latest_time",
                        "unknown time basis or inactive timestamp",
                    ));
                }
            };
            Ok(Some(CopyDirectorySummary {
                database: DatabaseId::new(
                    durable_steps::Uuid::parse_str(project.as_str())
                        .map_err(|error| corrupt("copy.project", error))?,
                )
                .map_err(|error| corrupt("copy.project", error))?,
                position: ReplicationPosition::new(
                    Epoch::new(decimal(&epoch, "copy.epoch")?)
                        .map_err(|error| corrupt("copy.epoch", error))?,
                    CommitSequence::new(decimal(&sequence, "copy.sequence")?),
                ),
                summary: BoardAnnotation::ProjectSummary { latest_record_at },
                acked_at: SyncTime::from_unix_millis(acked),
            }))
        }
        _ => Err(corrupt("copy.summary", "partial committed Copy summary")),
    }
}
