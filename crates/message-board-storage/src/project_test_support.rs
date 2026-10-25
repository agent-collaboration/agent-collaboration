//! Unit fixtures use the product schema and real library owner, with explicit provisional authority.
use crate::{ProjectStorageError, ProjectStore};
use board_fleet_sync::BoardProjectSchema;
use fleet_sync_lease::{LeaseHolder, LeasePolicy};
use fleet_sync_protocol::{DatabaseId, PeerId};
use fleet_sync_writer::{ReplicatedDatabase, WriterOptions};
use message_board::{ActorOrigin, MachineLabel, ProjectId, RecordPlacement, ServiceId};
use std::{path::Path, sync::Arc, time::Duration};

#[allow(clippy::unwrap_used)]
pub(crate) fn origin() -> ActorOrigin {
    ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
            .unwrap(),
        machine: MachineLabel::try_from("native storage fixture".to_owned()).unwrap(),
    }
}

pub(crate) fn placement(cursor: message_board::FeedCursor) -> RecordPlacement {
    RecordPlacement::Project(message_board::ProjectFeedEntry {
        cursor,
        origin: origin(),
    })
}

pub(crate) fn rejection()
-> Result<fleet_sync_protocol::GateReason, fleet_sync_protocol::TextValueError> {
    Ok(fleet_sync_protocol::GateReason {
        code: fleet_sync_protocol::GateReasonCode::new("board.projectRejected")?,
        detail: "native move fixture".to_owned(),
    })
}

impl ProjectStore<BoardProjectSchema> {
    pub(crate) async fn open(path: &Path, project: ProjectId) -> Result<Self, ProjectStorageError> {
        let database_id = DatabaseId::new(
            uuid::Uuid::parse_str(project.as_str())
                .map_err(|_| ProjectStorageError::InvalidSchema)?,
        )
        .map_err(|_| ProjectStorageError::InvalidSchema)?;
        let peer = PeerId::new(
            uuid::Uuid::parse_str("0192a1b2-c3d4-7000-8000-000000000099")
                .map_err(|_| ProjectStorageError::InvalidSchema)?,
        )
        .map_err(|_| ProjectStorageError::InvalidSchema)?;
        let policy = LeasePolicy {
            lease_duration: Duration::from_secs(14400),
            renew_interval: Duration::from_secs(900),
        };
        let database = Arc::new(
            ReplicatedDatabase::open_with(
                path,
                database_id,
                BoardProjectSchema::new().map_err(|_| ProjectStorageError::InvalidSchema)?,
                LeaseHolder::provisional(database_id, peer, policy),
                Arc::new(runtime_primitives::SystemClock),
                WriterOptions {
                    allow_provisional_writes: true,
                },
            )
            .await?,
        );
        Self::from_database(project, database).await
    }
}
