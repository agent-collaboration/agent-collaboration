//! Observe the existing importer through its public preparation and owning snapshots.
use board_legacy_upgrade::{PreparedUpgrade, UpgradeReport, prepare_sources};
use fleet_admission::ImportProvenance;
use fleet_sync_protocol::SnapshotPage;
use message_board::{HumanId, Identity, InboxReadMode, ProjectId, RecordKind};
use message_board_storage::{FeedPageBounds, FeedPageDirection};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
};

type ObservationResult<T> = Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObservationRequest {
    source: PathBuf,
    output: PathBuf,
    readers: Vec<Identity>,
    owner: HumanId,
    provenance: ImportProvenance,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InboxObservation {
    reader: Identity,
    mode: &'static str,
    post_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectObservation {
    project: ProjectId,
    pages: Vec<SnapshotPage>,
    inboxes: Vec<InboxObservation>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeObservation<'a> {
    report: &'a UpgradeReport,
    projects: Vec<ProjectObservation>,
}

async fn collect(prepared: &PreparedUpgrade, readers: &[Identity]) -> ObservationResult<Vec<u8>> {
    let mut projects = Vec::new();
    for project in &prepared.projects {
        let mut view = project.store.snapshot().await?;
        let mut inboxes = Vec::new();
        for reader in readers {
            for (mode, read_mode, direction) in [
                ("unread", InboxReadMode::Unread, FeedPageDirection::Forward),
                ("latest", InboxReadMode::Latest, FeedPageDirection::Backward),
            ] {
                let bounds = FeedPageBounds {
                    after: message_board::FeedPosition::ZERO,
                    through: view.high_water().position,
                    direction,
                    limit: 1000,
                    sweep: None,
                };
                let keys = view
                    .inbox_page_keys(&bounds, reader, read_mode, true)
                    .await?;
                if keys.len() >= 1000 {
                    return Err(io::Error::other(
                        "fixture exceeds the bounded observer inbox page",
                    )
                    .into());
                }
                inboxes.push(InboxObservation {
                    reader: reader.clone(),
                    mode,
                    post_ids: keys
                        .into_iter()
                        .filter(|key| key.reference.kind == RecordKind::Post)
                        .map(|key| key.reference.id.as_str().to_owned())
                        .collect(),
                });
            }
        }
        view.finish().await?;
        let mut snapshot = project.database.snapshot().await?;
        let mut pages = Vec::new();
        let mut cursor = None;
        loop {
            let page = snapshot.page(cursor).await?;
            cursor = page.next.clone();
            pages.push(page);
            if cursor.is_none() {
                break;
            }
        }
        snapshot.close().await?;
        projects.push(ProjectObservation {
            project: project.store.project_id().clone(),
            pages,
            inboxes,
        });
    }
    Ok(serde_json::to_vec_pretty(&NativeObservation {
        report: &prepared.report,
        projects,
    })?)
}

#[tokio::main]
async fn main() -> ObservationResult<()> {
    let request_path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| io::Error::other("usage: board-upgrade-native-observer REQUEST.json"))?;
    let request: ObservationRequest = serde_json::from_slice(&std::fs::read(request_path)?)?;
    let prepared = prepare_sources(&[request.source], request.provenance, request.owner).await?;
    let result = collect(&prepared, &request.readers).await;
    prepared.close().await;
    let bytes = result?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(request.output)?;
    output.write_all(&bytes)?;
    Ok(())
}
