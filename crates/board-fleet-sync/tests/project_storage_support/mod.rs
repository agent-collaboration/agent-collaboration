//! Shared real-library fixtures for the separate storage and cursor integration targets.
#![allow(dead_code, clippy::unwrap_used)]
use board_fleet_sync::BoardProjectSchema;
use fleet_sync_lease::{LeaseHolder, LeasePolicy};
use fleet_sync_protocol::{DatabaseId, PeerId};
use fleet_sync_writer::{ReplicatedDatabase, WriterOptions};
use message_board::*;
use message_board_storage::{ImportRowKeys, ImportRowKind, ProjectStore, StoreSchemaState};
use std::{path::Path, sync::Arc, time::Duration};

pub async fn open_project(
    path: &Path,
    project: &ProjectId,
    peer: PeerId,
) -> (
    Arc<ReplicatedDatabase<BoardProjectSchema>>,
    ProjectStore<BoardProjectSchema>,
) {
    let database_id =
        DatabaseId::new(durable_steps::Uuid::parse_str(project.as_str()).unwrap()).unwrap();
    let policy = LeasePolicy {
        lease_duration: Duration::from_secs(14400),
        renew_interval: Duration::from_secs(900),
    };
    let database = Arc::new(
        ReplicatedDatabase::open_with(
            path,
            database_id,
            BoardProjectSchema::new().unwrap(),
            LeaseHolder::provisional(database_id, peer, policy),
            Arc::new(runtime_primitives::SystemClock),
            WriterOptions {
                allow_provisional_writes: true,
            },
        )
        .await
        .unwrap(),
    );
    let store = ProjectStore::from_database(project.clone(), database.clone())
        .await
        .unwrap();
    assert!(matches!(
        store.schema_state(),
        StoreSchemaState::Ready { version: 1, .. }
    ));
    (database, store)
}
pub fn peer() -> PeerId {
    PeerId::new(durable_steps::Uuid::now_v7()).unwrap()
}
pub fn origin(number: u8, label: &str) -> ActorOrigin {
    ActorOrigin {
        installation: ServiceId::try_from(format!("00000000-0000-4000-8000-{number:012x}"))
            .unwrap(),
        machine: MachineLabel::try_from(label.to_owned()).unwrap(),
    }
}
pub fn reader(number: u8) -> Identity {
    serde_json::from_value(serde_json::json!({"kind":"session","session":{"endpoint":{"serviceId":format!("00000000-0000-4000-8000-{number:012x}"),"endpointId":"fixture"},"sessionId":"same-provider-session"}})).unwrap()
}
pub fn project_record(project: &ProjectId, source: &ActorOrigin) -> Record {
    Record::Project(ProjectState {
        envelope: RecordEnvelope {
            id: project.clone().into(),
            scope: RecordScope {
                workspace: WorkspaceId::generate(),
                board: Some(BoardId::generate()),
                project: Some(project.clone()),
            },
            actor: RecordedActor::declared(reader(1)),
            request: None,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: FeedCursor {
                    epoch: EpochNumber::INITIAL,
                    position: FeedPosition::ZERO,
                },
                origin: source.clone(),
            }),
            time: RecordTime::Router("2026-10-18T12:00:00Z".parse().unwrap()),
        },
        project_id: project.clone(),
        status: None,
        pinned_definition: None,
        revision: SubjectRevision::try_from(1).unwrap(),
        closed_at: None,
        description: Description::try_from(String::new()).unwrap(),
    })
}
pub fn revised(record: &Record, revision: u64) -> Record {
    let mut row = match record {
        Record::Project(row) => Some(row.clone()),
        _ => None,
    }
    .unwrap();
    row.revision = SubjectRevision::try_from(revision).unwrap();
    Record::Project(row)
}

pub fn accepted_entries(read: FeedRead) -> (Vec<FeedEntry>, FeedCursor) {
    match read {
        FeedRead::Entries {
            entries,
            covered_through,
            ..
        } => Some((entries, covered_through)),
        _ => None,
    }
    .unwrap()
}
pub fn mark(record: &Record, id: RecordId, through: FeedCursor) -> ReadMark {
    ReadMark {
        envelope: RecordEnvelope {
            id,
            position: RecordPlacement::Unsequenced,
            ..record.envelope().clone()
        },
        reader: reader(1),
        scope: ReadScope::Project {
            project_id: record.envelope().scope.project.clone().unwrap(),
        },
        through,
    }
}

pub const ACTOR_KEY: &str = "0192a1b2-c3d4-7000-8000-000000000001";
pub const LINE_KEY: &str = "0192a1b2-c3d4-7000-8000-000000000002";
pub const FEED_KEY: &str = "0192a1b2-c3d4-7000-8000-000000000003";
pub struct FixedImportKeys {
    pub invalid_actor: bool,
}
impl ImportRowKeys for FixedImportKeys {
    fn row_key(&self, kind: ImportRowKind, _: &[&str]) -> String {
        match kind {
            ImportRowKind::Actor if self.invalid_actor => "00000000-0000-4000-8000-000000000001",
            ImportRowKind::Actor => ACTOR_KEY,
            ImportRowKind::EpochLine => LINE_KEY,
            ImportRowKind::FeedEntry => FEED_KEY,
            _ => "missing fixture key",
        }
        .to_owned()
    }
}

pub async fn open_granted_project(
    path: &Path,
    project: &ProjectId,
    peer: PeerId,
    epoch: u64,
) -> (
    Arc<ReplicatedDatabase<BoardProjectSchema>>,
    ProjectStore<BoardProjectSchema>,
) {
    use runtime_primitives::Clock;
    let database_id =
        DatabaseId::new(durable_steps::Uuid::parse_str(project.as_str()).unwrap()).unwrap();
    let duration = Duration::from_secs(14400);
    let policy = LeasePolicy {
        lease_duration: duration,
        renew_interval: Duration::from_secs(900),
    };
    let clock = Arc::new(runtime_primitives::SystemClock);
    let started = clock.now();
    let mut holder = LeaseHolder::provisional(database_id, peer, policy);
    // A typed grant is the fixture input. Registry, transfer and receiver behavior are separate proofs.
    holder
        .observe_registration(
            fleet_sync_protocol::LeaseStatus {
                database: database_id,
                epoch: fleet_sync_protocol::Epoch::new(epoch).unwrap(),
                holder: peer,
                state: fleet_sync_protocol::LeaseState::Held,
                expires_at: started.after(duration),
                remaining_ms: 14_400_000,
            },
            started,
        )
        .unwrap();
    let database = Arc::new(
        ReplicatedDatabase::open_with(
            path,
            database_id,
            BoardProjectSchema::new().unwrap(),
            holder,
            clock,
            WriterOptions::default(),
        )
        .await
        .unwrap(),
    );
    let store = ProjectStore::from_database(project.clone(), database.clone())
        .await
        .unwrap();
    (database, store)
}
