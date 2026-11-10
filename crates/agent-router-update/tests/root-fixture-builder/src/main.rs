//! Writes a populated Router root through the old release's own store writers.
//!
//! The Fleet cache ends with its first feed page in the database file and a second
//! committed page only in its write-ahead log: the builder exits without closing
//! the store, as a killed Host would.
use fleet_admission::{FleetFeedEntry, FleetOperation, FleetRequest};
use message_board::*;
use message_board_storage::{CursorSigningKey, CursorStore, FleetCache, FleetOutbox};
use std::{error::Error, path::PathBuf};

type BuilderResult<T = ()> = Result<T, Box<dyn Error>>;

const WORKSPACE_IN_DATABASE: &str = "019f0000-0000-7000-8000-00000000d301";
const WORKSPACE_ONLY_IN_LOG: &str = "019f0000-0000-7000-8000-00000000d302";
const OUTBOX_REQUEST: &str = "019f0000-0000-7000-8000-00000000d311";
const OUTBOX_WORKSPACE: &str = "019f0000-0000-7000-8000-00000000d312";
const DECIDED_REQUEST: &str = "019f0000-0000-7000-8000-00000000d313";
const DECIDED_WORKSPACE: &str = "019f0000-0000-7000-8000-00000000d314";
const CREATION_REQUEST: &str = "019f0000-0000-7000-8000-00000000d321";
const CREATION_PROJECT: &str = "019f0000-0000-7000-8000-00000000d322";
const CREATION_BOARD: &str = "019f0000-0000-7000-8000-00000000d323";
const CREATION_WORKSPACE: &str = "019f0000-0000-7000-8000-00000000d324";
/// The decided request's settle time, a literal so tests can compare it exactly.
const DECIDED_AT_MS: i64 = 1_793_000_000_000;
const ORIGIN_SERVICE: &str = "550e8400-e29b-41d4-a716-44665544d300";

fn owner() -> BuilderResult<RecordedActor> {
    Ok(RecordedActor::declared(Identity::Human {
        human_id: HumanId::try_from("fixture owner".to_owned())?,
    }))
}

fn workspace_entry(id: &str, name: &str, position: u64) -> BuilderResult<FleetFeedEntry> {
    let workspace = WorkspaceId::try_from(id.to_owned())?;
    let position = FleetPosition::try_from(position)?;
    let record = Record::Workspace(Workspace {
        envelope: RecordEnvelope {
            id: RecordId::try_from(workspace.as_str().to_owned())?,
            scope: RecordScope {
                workspace: workspace.clone(),
                board: None,
                project: None,
            },
            actor: owner()?,
            request: None,
            position: RecordPlacement::Fleet(message_board::FleetFeedEntry {
                position,
                origin: ActorOrigin {
                    installation: ServiceId::try_from(ORIGIN_SERVICE.to_owned())?,
                    machine: MachineLabel::try_from("fixture".to_owned())?,
                },
            }),
            time: RecordTime::LegacyUnknown,
        },
        workspace_id: workspace,
        name: ResourceName::try_from(name.to_owned())?,
        description: Description::try_from("populated root fixture".to_owned())?,
        state: WorkspaceState::Active,
        repositories: vec![],
    });
    Ok(FleetFeedEntry { position, record })
}

#[tokio::main]
async fn main() -> BuilderResult {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("output Router root required")?);
    if root.exists() {
        return Err("refuse to overwrite fixture".into());
    }
    let communication = root.join("agent-communication");
    std::fs::create_dir_all(&communication)?;

    let outbox = FleetOutbox::open(&communication.join("fleet-outbox.sqlite")).await?;
    outbox
        .enqueue(&FleetRequest {
            request_id: RequestId::try_from(OUTBOX_REQUEST.to_owned())?,
            workspace: WorkspaceId::try_from(OUTBOX_WORKSPACE.to_owned())?,
            actor: owner()?,
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: FleetOperation::WorkspaceCreate {
                name: ResourceName::try_from("Pending workspace".to_owned())?,
                description: Description::try_from("waiting for central".to_owned())?,
            },
        })
        .await?;
    let decided = FleetRequest {
        request_id: RequestId::try_from(DECIDED_REQUEST.to_owned())?,
        workspace: WorkspaceId::try_from(DECIDED_WORKSPACE.to_owned())?,
        actor: owner()?,
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("Decided workspace".to_owned())?,
            description: Description::try_from("central accepted it".to_owned())?,
        },
    };
    outbox.enqueue(&decided).await?;
    outbox
        .mark_decided(
            &decided.request_id,
            &RequestDecision::Accepted {
                effects: DecisionEffectsPage::from_full(
                    Vec::new(),
                    &DecisionEffectsHeader::from_full(&[]),
                    None,
                )?,
                change: ChangeOutcome::Changed,
            },
            DECIDED_AT_MS,
        )
        .await?;
    let creation_input = ProjectCreateRequest {
        request_id: RequestId::try_from(CREATION_REQUEST.to_owned())?,
        protocol_version: ProtocolVersion::CURRENT,
        project_id: ProjectId::try_from(CREATION_PROJECT.to_owned())?,
        board_id: BoardId::try_from(CREATION_BOARD.to_owned())?,
        title: ResourceName::try_from("Queued project".to_owned())?,
        description: Some(Description::try_from("created through the outbox".to_owned())?),
        actor: Identity::Human {
            human_id: HumanId::try_from("fixture owner".to_owned())?,
        },
        acting_for: None,
    };
    let creation_directory = FleetRequest {
        request_id: creation_input.request_id.clone(),
        workspace: WorkspaceId::try_from(CREATION_WORKSPACE.to_owned())?,
        actor: RecordedActor::declared(creation_input.actor.clone()),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::ProjectDirectoryCreate {
            project: creation_input.project_id.clone(),
            board: creation_input.board_id.clone(),
            title: creation_input.title.clone(),
        },
    };
    outbox
        .enqueue_creation(&creation_directory, &creation_input)
        .await?;
    outbox.close().await;

    let cursors = CursorStore::open(&communication.join("board-cursors.sqlite")).await?;
    cursors
        .get_or_install_key(&CursorSigningKey::try_from(vec![0xd3; 32])?)
        .await?;
    cursors.close().await;

    let cache_path = communication.join("fleet-cache.sqlite");
    let cache = FleetCache::open(&cache_path).await?;
    cache
        .apply_feed(
            FleetPosition::ZERO,
            &[workspace_entry(WORKSPACE_IN_DATABASE, "Checkpointed workspace", 1)?],
        )
        .await?;
    cache.close().await;
    // Turso leaves every page in the log on close; move the first page into the
    // database file so only the second page is the log-only one.
    checkpoint(&cache_path).await?;

    let killed_writer = FleetCache::open(&cache_path).await?;
    killed_writer
        .apply_feed(
            FleetPosition::try_from(1)?,
            &[workspace_entry(WORKSPACE_ONLY_IN_LOG, "Write-ahead log workspace", 2)?],
        )
        .await?;
    // Exit without closing, so the committed page is never checkpointed.
    std::process::exit(0);
}

async fn checkpoint(path: &std::path::Path) -> BuilderResult {
    use sqlx::{ConnectOptions, Connection};
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .connect()
        .await?;
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    Ok(())
}
