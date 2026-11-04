//! Pin one selected native writer's unchanged rows under the declared Host lock.
use crate::{OfflineHost, PreparedSnapshotPage, SnapshotTransportError, reframe_import_page};
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema, BoardProjectSchemaError};
use fleet_sync_lease::LeaseHolder;
use fleet_sync_protocol::{DatabaseId, Epoch, PeerId, SnapshotCursor, SnapshotManifest};
use fleet_sync_writer::{ReplicatedDatabase, RetainedFile, WriterOptions, WriterSnapshot};
use message_board::{
    EpochLine, EpochNumber, FeedCursor, FeedPosition, FeedRead, ProjectId, Record, RecordTime,
};
use message_board_storage::{ProjectStore, StoreSchemaState};
use runtime_primitives::SystemClock;
use std::{
    collections::BTreeSet,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct SelectedWriterFile {
    pub project: ProjectId,
    pub peer: PeerId,
    pub file_epoch: Epoch,
}

pub struct NativeSourcePreflight {
    pub selected: SelectedWriterFile,
    pub application_epoch: EpochNumber,
    pub high_water: FeedCursor,
    pub line_two: Option<EpochLine>,
    pub latest_record_at: RecordTime,
    pub manifest: SnapshotManifest,
}

#[derive(Debug, thiserror::Error)]
pub enum NativeSourceError {
    #[error("selected writer differs from the declared offline Host")]
    PeerMismatch,
    #[error("selected source must remain inside its owned project directory")]
    UnsafePath,
    #[error("selected native source identity or file epoch differs")]
    FileIdentityMismatch,
    #[error("source application history exceeds the supported import epoch")]
    UnsupportedApplicationEpoch { epoch: EpochNumber },
    #[error("source project history is invalid")]
    InvalidHistory,
    #[error("source project has no recorded content")]
    EmptyProject,
    #[error("the closed retained reader still holds native admission")]
    ReaderStillAdmitted,
    #[error("cannot inspect the selected native source")]
    Writer(#[from] fleet_sync_writer::OpenError),
    #[error("source project data is invalid")]
    Project(#[from] message_board_storage::ProjectStorageError),
    #[error("source schema is unavailable")]
    Schema(#[from] BoardProjectSchemaError),
    #[error("cannot read source history metadata")]
    Sql(#[from] sqlx::Error),
    #[error("cannot inspect the selected source path")]
    Io(#[from] std::io::Error),
    #[error("cannot reframe the native source page")]
    Transport(#[from] SnapshotTransportError),
}

enum SourceCursor {
    Start,
    After(SnapshotCursor),
    End,
}

pub struct NativeProjectSnapshot<'host> {
    _host: &'host mut OfflineHost,
    database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
    snapshot: WriterSnapshot,
    preflight: NativeSourcePreflight,
    cursor: SourceCursor,
}

impl<'host> NativeProjectSnapshot<'host> {
    pub async fn open(
        host: &'host mut OfflineHost,
        selected: SelectedWriterFile,
    ) -> Result<Self, NativeSourceError> {
        let path = selected_path(host, &selected)?;
        let database_id = DatabaseId::try_from(selected.project.as_str().to_owned())
            .map_err(|_| NativeSourceError::FileIdentityMismatch)?;
        let retained = RetainedFile::open_read_only(&path, BoardProjectSchema::new()?).await?;
        let origin = retained.origin();
        if origin.database != database_id || origin.epoch != selected.file_epoch {
            return Err(NativeSourceError::FileIdentityMismatch);
        }
        // Unsupported application epochs refuse before a writable owner opens.
        let application_epoch = read_application_epoch(&retained).await?;
        drop(retained);
        let database = Arc::new(open_after_retained(&path, database_id, selected.peer).await?);
        let inspected = inspect_source(&database, selected, application_epoch).await;
        match inspected {
            Ok((snapshot, preflight)) => Ok(Self {
                _host: host,
                database,
                snapshot,
                preflight,
                cursor: SourceCursor::Start,
            }),
            Err(error) => {
                database.close().await;
                Err(error)
            }
        }
    }

    pub fn preflight(&self) -> &NativeSourcePreflight {
        &self.preflight
    }

    pub async fn next_page(&mut self) -> Result<Option<PreparedSnapshotPage>, NativeSourceError> {
        let cursor = match &self.cursor {
            SourceCursor::Start => None,
            SourceCursor::After(cursor) => Some(cursor.clone()),
            SourceCursor::End => return Ok(None),
        };
        let page = self.snapshot.page(cursor).await?;
        let prepared = reframe_import_page(page, &self.preflight.manifest)?;
        self.cursor = prepared
            .source_next
            .clone()
            .map_or(SourceCursor::End, SourceCursor::After);
        Ok(Some(prepared))
    }

    pub async fn close(self) -> Result<(), NativeSourceError> {
        let closed = self.snapshot.close().await;
        self.database.close().await;
        closed.map_err(Into::into)
    }
}

fn selected_path(
    host: &OfflineHost,
    selected: &SelectedWriterFile,
) -> Result<PathBuf, NativeSourceError> {
    if selected.peer != host.peer() {
        return Err(NativeSourceError::PeerMismatch);
    }
    let projects = host.collaboration_directory().join("projects");
    let project = projects.join(selected.project.as_str());
    for directory in [&projects, &project] {
        let metadata = std::fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.uid() != nix::unistd::geteuid().as_raw() {
            return Err(NativeSourceError::UnsafePath);
        }
    }
    let path = project.join(format!("e{}.sqlite", selected.file_epoch.value()));
    let metadata = std::fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.uid() != nix::unistd::geteuid().as_raw() {
        return Err(NativeSourceError::UnsafePath);
    }
    Ok(path)
}

struct StoredEpochNumber {
    epoch: i64,
    storage_type: String,
}

async fn read_application_epoch(retained: &RetainedFile) -> Result<EpochNumber, NativeSourceError> {
    let mut read = retained.reader().begin_read().await?;
    let inspected = async {
        // At most two distinct positive epochs fit A0. A third row is enough
        // to reject an invalid duplicate/range without scanning unbounded history.
        let rows = sqlx_turso::query_as!(StoredEpochNumber,
            r#"SELECT epoch AS "epoch!: i64", typeof(epoch) AS "storage_type!: String" FROM epoch_lines ORDER BY epoch DESC LIMIT 3"#)
            .fetch_all(&mut read).await?;
        if rows.is_empty() {
            return Err(NativeSourceError::EmptyProject);
        }
        let mut seen = BTreeSet::new();
        let mut greatest = EpochNumber::INITIAL;
        for row in rows {
            if row.storage_type != "integer" {
                return Err(NativeSourceError::InvalidHistory);
            }
            let epoch = u64::try_from(row.epoch).ok().and_then(|value| EpochNumber::try_from(value).ok())
                .ok_or(NativeSourceError::InvalidHistory)?;
            if epoch.get() > 2 {
                return Err(NativeSourceError::UnsupportedApplicationEpoch { epoch });
            }
            if !seen.insert(epoch) {
                return Err(NativeSourceError::InvalidHistory);
            }
            greatest = greatest.max(epoch);
        }
        Ok(greatest)
    }.await;
    let closed = read.close().await;
    let epoch = inspected?;
    closed?;
    Ok(epoch)
}

async fn open_after_retained(
    path: &Path,
    database: DatabaseId,
    peer: PeerId,
) -> Result<ReplicatedDatabase<BoardProjectSchema>, NativeSourceError> {
    // SQLx can still be returning a closed read transaction. Wait on the SDK's
    // physical admission result, matching its retained-reader lifetime contract.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ReplicatedDatabase::open_with(
                path,
                database,
                BoardProjectSchema::new()?,
                LeaseHolder::provisional(database, peer, BOARD_LEASE_POLICY),
                Arc::new(SystemClock),
                WriterOptions {
                    allow_provisional_writes: false,
                },
            )
            .await
            {
                Err(fleet_sync_writer::OpenError::FileInUse) => tokio::task::yield_now().await,
                result => return result.map_err(NativeSourceError::from),
            }
        }
    })
    .await
    .map_err(|_| NativeSourceError::ReaderStillAdmitted)?
}

async fn inspect_source(
    database: &Arc<ReplicatedDatabase<BoardProjectSchema>>,
    selected: SelectedWriterFile,
    application_epoch: EpochNumber,
) -> Result<(WriterSnapshot, NativeSourcePreflight), NativeSourceError> {
    let store = ProjectStore::from_database(selected.project.clone(), database.clone()).await?;
    if matches!(store.schema_state(), StoreSchemaState::ReadOnly { .. }) {
        return Err(NativeSourceError::InvalidHistory);
    }
    let (high_water, line_two, latest_record_at) =
        project_history(&store, &selected.project, application_epoch).await?;
    drop(store);
    let mut snapshot = database.snapshot().await?;
    let manifest = snapshot.manifest().await?;
    if manifest.database.to_string() != selected.project.as_str()
        || manifest.position.epoch != selected.file_epoch
    {
        return Err(NativeSourceError::FileIdentityMismatch);
    }
    Ok((
        snapshot,
        NativeSourcePreflight {
            selected,
            application_epoch,
            high_water,
            line_two,
            latest_record_at,
            manifest,
        },
    ))
}

async fn project_history(
    store: &ProjectStore<BoardProjectSchema>,
    project: &ProjectId,
    application_epoch: EpochNumber,
) -> Result<(FeedCursor, Option<EpochLine>, RecordTime), NativeSourceError> {
    let mut read = store.snapshot().await?;
    let inspected = async {
        let Some(Record::Project(_)) = read.record(&project.clone().into()).await? else {
            return Err(NativeSourceError::InvalidHistory);
        };
        let high = read.high_water();
        if high.position == FeedPosition::ZERO {
            return Err(NativeSourceError::EmptyProject);
        }
        read.validate_feed_range(FeedPosition::ZERO, high.position)
            .await?;
        let latest_line = read
            .epoch_line(application_epoch)
            .await?
            .ok_or(NativeSourceError::InvalidHistory)?;
        if latest_line.inherited_through > high.position {
            return Err(NativeSourceError::InvalidHistory);
        }
        let second = EpochNumber::try_from(2_u64).map_err(|_| NativeSourceError::InvalidHistory)?;
        let line_two = read.epoch_line(second).await?;
        if line_two
            .as_ref()
            .is_some_and(|line| line.inherited_through > high.position)
        {
            return Err(NativeSourceError::InvalidHistory);
        }
        let before = FeedPosition::try_from(high.position.get() - 1)
            .map_err(|_| NativeSourceError::InvalidHistory)?;
        let after = read.feed_cursor_or_start(before).await?;
        let FeedRead::Entries {
            entries,
            covered_through,
            ..
        } = read.feed_after(after, 1, true).await?
        else {
            return Err(NativeSourceError::InvalidHistory);
        };
        let latest = entries.last().ok_or(NativeSourceError::InvalidHistory)?;
        if covered_through != high {
            return Err(NativeSourceError::InvalidHistory);
        }
        Ok((high, line_two, latest.time))
    }
    .await;
    let closed = read.finish().await;
    let summary = inspected?;
    closed?;
    Ok(summary)
}

#[cfg(test)]
#[path = "native_project_snapshot_test.rs"]
mod tests;
