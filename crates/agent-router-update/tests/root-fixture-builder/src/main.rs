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
