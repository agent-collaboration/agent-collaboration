#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Measure actual JSON pieces and snapshot pages; count-based paging is unchanged.
#![allow(clippy::panic_in_result_fn)]
#[path = "support/accepted_snapshot.rs"]
pub mod accepted_snapshot;
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/project_description.rs"]
mod project_description;
use board_fixture::{BoardFixture, TestError, accepted};
use board_fleet_sync::{BoardCaptureBookkeeping, BoardPieceAnnotation};
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use fleet_sync_protocol::{CopyRole, Piece, ReceiverId, ReplicationAnswer, SnapshotPage};
use fleet_sync_writer::{ReplicationSender, ReplicationTransport, SendFailure, SenderOptions};
use message_board::*;
use project_description::{create_project, description_with_control_characters, project_state};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

const VERSIONS: u32 = 260;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct EncodedSizes {
    versions: u32,
    description_bytes: usize,
    pieces: usize,
    largest_piece_bytes: usize,
    snapshot_pages: usize,
    history_rows: usize,
    largest_snapshot_page_bytes: usize,
    largest_history_page_bytes: usize,
}

#[derive(Default)]
struct EncodingCopy {
    sizes: Mutex<EncodedSizes>,
}

fn encoding_failure(error: serde_json::Error) -> SendFailure {
    SendFailure::Unavailable {
        message: format!("fixture JSON codec: {error}"),
    }
}

impl ReplicationTransport<BoardConsumerProtocol> for EncodingCopy {
    async fn send_piece(
        &self,
        _: ReceiverId,
        piece: Piece<BoardAnnotation>,
    ) -> Result<ReplicationAnswer, SendFailure> {
        let bytes = serde_json::to_vec(&piece).map_err(encoding_failure)?;
        let decoded: Piece<BoardAnnotation> =
            serde_json::from_slice(&bytes).map_err(encoding_failure)?;
        assert_eq!(
            decoded, piece,
            "the actual piece codec changed captured values"
        );
        {
            let mut sizes = self.sizes.lock().await;
            sizes.pieces += 1;
            sizes.largest_piece_bytes = sizes.largest_piece_bytes.max(bytes.len());
        }
        // The acknowledgement stands in for copy policy. Native capture, piece
        // construction and the validated codec above remain real.
        Ok(match &piece.part {
            Some(part) if part.index + 1 < part.count => ReplicationAnswer::PartStaged {
                transaction: part.transaction,
                index: part.index,
                acked: piece.continues_from,
            },
            _ => ReplicationAnswer::Applied {
                through: piece.through,
            },
        })
    }
}

#[tokio::test]
async fn maximum_descriptions_survive_encoded_capture_and_populated_history_snapshots()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let initial = description_with_control_characters(0);
    assert_eq!(initial.len(), 16 * 1024);
    let (project, _) = create_project(&source, Some(&initial)).await?;
    let writer = BoardFixture::start().await?;
    let (inherited, _epoch) =
        accepted_snapshot::bootstrap_project(&source, &writer, &project).await?;
    let database = writer._registry.database(&project).await?;
    let snapshot = database.snapshot().await?;
    let start = snapshot.position();
    snapshot.close().await?;
    assert_eq!(
        project_state(&writer, &project).await?.revision,
        SubjectRevision::INITIAL
    );
    for version in 1..=VERSIONS {
        let description = description_with_control_characters(version);
        assert_eq!(description.len(), 16 * 1024);
        let request = writer.request(
            &project,
            json!({
                "kind":"projectDescribe","description":description,"expectedRevision":version - 1
            }),
        );
        accepted(&writer.api.call("project_describe", request).await?);
    }
    let snapshot = database.snapshot().await?;
    let tail = snapshot.position();
    snapshot.close().await?;
    assert_ne!(
        tail, start,
        "the native writer must actually capture these versions"
    );
    let transport = Arc::new(EncodingCopy::default());
    let sender = ReplicationSender::new(
        database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        transport.clone(),
        SenderOptions::default(),
    );
    let copy = ReceiverId::new(uuid::Uuid::now_v7())?;
    sender
        .add_copy(copy, CopyRole::Authoritative, start)
        .await?;
    for _ in 0..=(VERSIONS * 2 + 10) {
        if sender
            .copies()
            .await?
            .iter()
            .any(|state| state.copy == copy && state.acked == tail)
        {
            break;
        }
        sender.replicate_once(copy).await?;
    }
    assert!(
        sender
            .copies()
            .await?
            .iter()
            .any(|state| state.copy == copy && state.acked == tail),
        "encoded replication did not reach its own captured tail"
    );

    // This is the real populated native snapshot, with every historical state
    // in project_feed. Measure full page JSON, including its schema statements.
    let mut snapshot = database.snapshot().await?;
    let mut cursor = None;
    let mut page_count = 0;
    let mut history_rows = 0;
    let mut largest_page = 0;
    let mut largest_history_page = 0;
    loop {
        let page = snapshot.page(cursor).await?;
        let encoded = serde_json::to_vec(&page)?;
        let decoded: SnapshotPage = serde_json::from_slice(&encoded)?;
        assert_eq!(decoded, page, "snapshot page codec changed native values");
        page_count += 1;
        largest_page = largest_page.max(encoded.len());
        if page.table == "project_feed" {
            history_rows += page.rows.len();
            largest_history_page = largest_history_page.max(encoded.len());
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    snapshot.close().await?;
    assert!(
        history_rows >= usize::try_from(VERSIONS)? * 2,
        "history snapshot was not populated with state versions and their ledgers"
    );
    assert!(largest_history_page > 16 * 1024);

    // The owning library also stages, verifies and publishes the populated
    // snapshot into another native writer, rather than checking only JSON.
    let restored = BoardFixture::start().await?;
    accepted_snapshot::bootstrap_project(&writer, &restored, &project).await?;
    assert_eq!(
        project_state(&restored, &project)
            .await?
            .description
            .as_str(),
        description_with_control_characters(VERSIONS)
    );
    let store = restored
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("restored project")?;
    let mut read = store.snapshot().await?;
    let mut after = inherited;
    let mut next_version = 1;
    loop {
        let FeedRead::Entries { entries, .. } = read.feed_after(after, 100, true).await? else {
            return Err("accepted description history became superseded".into());
        };
        if entries.is_empty() {
            break;
        }
        for entry in entries {
            assert!(
                entry.position > after.position,
                "history page did not advance"
            );
            after = FeedCursor {
                epoch: entry.epoch,
                position: entry.position,
            };
            if let FeedContent::Shown(record) = entry.content
                && let Record::Project(state) = record.as_ref()
            {
                assert_eq!(
                    state.description.as_str(),
                    description_with_control_characters(next_version)
                );
                next_version += 1;
            }
        }
    }
    assert_eq!(
        next_version,
        VERSIONS + 1,
        "a historical state version disappeared"
    );
    read.finish().await?;
    {
        let mut sizes = transport.sizes.lock().await;
        sizes.versions = VERSIONS;
        sizes.description_bytes = initial.len();
        sizes.snapshot_pages = page_count;
        sizes.history_rows = history_rows;
        sizes.largest_snapshot_page_bytes = largest_page;
        sizes.largest_history_page_bytes = largest_history_page;
        assert!(sizes.pieces > 0 && sizes.largest_piece_bytes > 16 * 1024);
        eprintln!(
            "PROJECT_DESCRIPTION_ENCODED_SIZES {}",
            serde_json::to_string(&*sizes)?
        );
    }
    drop(store);
    drop(sender);
    drop(database);
    source.stop().await?;
    writer.stop().await?;
    restored.stop().await
}
