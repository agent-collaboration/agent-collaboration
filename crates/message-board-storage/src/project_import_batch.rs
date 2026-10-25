//! Privileged import batches reuse the live typed row writers and their validation.
use crate::{ImportRowKeys, ProjectStorageError, ProjectWriteTransaction};
use message_board::{FeedCursor, Identity, Participant, ProjectId, ReadMark, Record, ThreadId};
use std::{sync::Arc, time::Instant};

pub struct ImportRecord {
    pub record: Record,
    pub participants: Vec<(ThreadId, Participant)>,
}
pub struct ImportWatch {
    pub reader: Identity,
    pub thread: Option<ThreadId>,
    pub starts_after: FeedCursor,
    pub active: bool,
}
#[derive(Default)]
pub struct ProjectImportBatch {
    pub records: Vec<ImportRecord>,
    pub read_marks: Vec<ReadMark>,
    pub watches: Vec<ImportWatch>,
}

pub async fn apply_project_import(
    project: ProjectId,
    transaction: fleet_sync_writer::WriteTransaction,
    provenance: fleet_admission::ImportProvenance,
    row_keys: Arc<dyn ImportRowKeys>,
    batch: ProjectImportBatch,
) -> Result<(), ProjectStorageError> {
    let mut write =
        ProjectWriteTransaction::for_import(project, transaction, provenance, row_keys).await?;
    let started = Instant::now();
    let outcome = async {
        for (index, entry) in batch.records.into_iter().enumerate() {
            let expected = entry
                .record
                .envelope()
                .position
                .project_cursor()
                .ok_or(ProjectStorageError::InvalidFeed)?;
            if write.append_record(entry.record).await? != expected {
                return Err(ProjectStorageError::InvalidFeed);
            }
            for (thread, participant) in entry.participants {
                write.put_participant(&thread, &participant).await?;
            }
            if (index + 1).is_multiple_of(1000) {
                tracing::info!(target: "message_board_storage::import_progress", phase = "staging", rows = index + 1, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        for mark in batch.read_marks {
            write.write_reader_mark(mark).await?;
        }
        for watch in batch.watches {
            match watch.thread {
                Some(thread) => {
                    write
                        .set_thread_watch(&thread, &watch.reader, watch.starts_after, watch.active)
                        .await?
                }
                None => {
                    write
                        .set_project_watch(&watch.reader, watch.starts_after, watch.active)
                        .await?
                }
            }
        }
        Ok(())
    }
    .await;
    match outcome {
        Ok(()) => write.commit().await,
        Err(error) => {
            write.rollback().await?;
            Err(error)
        }
    }
}
