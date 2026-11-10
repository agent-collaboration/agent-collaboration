//! Read ArchiveSeen and sender progress through public libraries on a fresh synthetic A cut.
use board_fleet_sync::BoardProjectSchema;
use collaboration_runtime::{NativeProjectStore, project_replication_cursor};
use fleet_sync_lease::{LeaseHolder, LeasePolicy};
use fleet_sync_protocol::{DatabaseId, NoConsumer, PeerId, Piece, ReceiverId, ReplicationAnswer};
use fleet_sync_writer::{
    ReplicatedDatabase, ReplicationSender, ReplicationTransport, SendFailure, SenderOptions,
};
use message_board::{BoardId, FactKind, LifecycleFact, ProjectId, Record, RecordKind};
use sha2::{Digest, Sha256};
use std::{os::unix::fs::MetadataExt, path::Path, sync::Arc, time::Duration};

type ObservationResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct NoNetwork;
impl ReplicationTransport<NoConsumer> for NoNetwork {
    async fn send_piece(
        &self,
        _: ReceiverId,
        _: Piece<()>,
    ) -> Result<ReplicationAnswer, SendFailure> {
        Err(SendFailure::Unavailable {
            message: "archive observer starts no network".to_owned(),
        })
    }
}
fn hash(path: &Path) -> ObservationResult<String> {
    Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
}
fn qualify_cut(scratch: &Path, untouched: &Path) -> ObservationResult<()> {
    let a = std::fs::symlink_metadata(scratch)?;
    let b = std::fs::symlink_metadata(untouched)?;
    let parent = scratch.parent().ok_or("cut parent missing")?;
    let directory = std::fs::symlink_metadata(parent)?;
    if !scratch.is_absolute()
        || !untouched.is_absolute()
        || std::fs::canonicalize(scratch)? != scratch
        || std::fs::canonicalize(untouched)? != untouched
        || scratch.file_name().is_none_or(|name| name != "A.sqlite")
        || untouched.file_name().is_none_or(|name| name != "B.sqlite")
        || untouched.parent() != Some(parent)
        || !a.is_file()
        || !b.is_file()
        || a.mode() & 0o777 != 0o600
        || b.mode() & 0o777 != 0o400
        || directory.mode() & 0o777 != 0o700
        || a.uid() != rustix::process::getuid().as_raw()
        || b.uid() != a.uid()
        || directory.uid() != a.uid()
        || (a.dev(), a.ino()) == (b.dev(), b.ino())
        || scratch
            .components()
            .any(|part| part.as_os_str() == ".codex-router")
    {
        return Err("fresh canonical owner-private A/B cut required".into());
    }
    Ok(())
}
async fn observe(
    scratch: &Path,
    untouched: &Path,
    project: ProjectId,
    board: BoardId,
    peer: PeerId,
) -> ObservationResult<serde_json::Value> {
    qualify_cut(scratch, untouched)?;
    let b_before = hash(untouched)?;
    let a_before = hash(scratch)?;
    let database: DatabaseId = serde_json::from_value(serde_json::json!(project))?;
    // This reader's scope is the newly created N=1 synthetic archive project only.
    let holder = LeaseHolder::provisional(
        database,
        peer,
        LeasePolicy {
            lease_duration: Duration::from_secs(240),
            renew_interval: Duration::from_secs(60),
        },
    );
    let writer = Arc::new(
        ReplicatedDatabase::open(scratch, database, BoardProjectSchema::new()?, holder).await?,
    );
    let result = async {
        let registration = writer.registration_outcome().await?;
        let fleet_sync_writer::WriterRegistrationOutcome::Ready { lease } = &registration else {
            return Err("synthetic writer has no ready registration".into());
        };
        if lease.database != database || lease.holder != peer || lease.epoch.value() != 1 {
            return Err("synthetic N=1 writer identity mismatch".into());
        }
        let store = NativeProjectStore::from_database(project.clone(), writer.clone()).await?;
        let mut snapshot = store.snapshot().await?;
        let high_water = snapshot.high_water();
        let records = snapshot.records(RecordKind::Fact).await?;
        snapshot.finish().await?;
        let facts = records.into_iter().filter_map(|record| match record {
            Record::Fact(fact) if matches!(&fact.kind, FactKind::Lifecycle(LifecycleFact::ArchiveSeen { board: actual }) if actual == &board) => Some(fact),
            _ => None,
        }).collect::<Vec<_>>();
        let sender = ReplicationSender::new(writer.clone(), Arc::new(()), Arc::new(()), Arc::new(NoNetwork), SenderOptions::default());
        let copies = sender.copies().await?;
        let mut acknowledgements = Vec::new();
        for copy in &copies {
            acknowledgements.push(serde_json::json!({
                "copy": copy.copy, "role": copy.role, "senderAck": copy.acked,
                "acknowledgedFeed": project_replication_cursor(&store, copy.acked, high_water.epoch).await?,
            }));
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(serde_json::json!({
            "kind": "archiveProgressObserved", "project": project, "board": board,
            "registration": registration, "highWater": high_water,
            "archiveFacts": facts, "copies": copies, "acknowledgements": acknowledgements,
            "privateSqlUsed": false, "networkStarted": false, "liveProjectOpened": false,
        }))
    }.await;
    writer.close().await;
    let b_after = hash(untouched)?;
    if b_before != b_after {
        return Err("sealed B changed".into());
    }
    let mut observation = result?;
    let object = observation
        .as_object_mut()
        .ok_or("observation envelope missing")?;
    object.insert("untouchedBeforeSha256".into(), b_before.into());
    object.insert("untouchedAfterSha256".into(), b_after.into());
    object.insert("scratchBeforeSha256".into(), a_before.into());
    object.insert("scratchAfterSha256".into(), hash(scratch)?.into());
    Ok(observation)
}
#[tokio::main]
async fn main() -> ObservationResult<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let [scratch, untouched, project, board, peer] = arguments.as_slice() else {
        return Err("usage: archive_progress_observation A.sqlite B.sqlite PROJECT BOARD PEER (fresh synthetic N=1 cut only)".into());
    };
    let parse = |value: &str| serde_json::Value::String(value.to_owned());
    let observed = observe(
        Path::new(scratch),
        Path::new(untouched),
        serde_json::from_value(parse(project))?,
        serde_json::from_value(parse(board))?,
        serde_json::from_value(parse(peer))?,
    )
    .await?;
    println!("{}", serde_json::to_string(&observed)?);
    Ok(())
}

#[cfg(test)]
#[path = "archive_progress_test.rs"]
mod tests;
