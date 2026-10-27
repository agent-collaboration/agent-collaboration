//! Test-only stdin queries drive real project copies and observe committed Local cancellation.
use codex_router_host::CollaborationRuntime;
use fleet_sync_protocol::ItemId;
use serde::Deserialize;
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ProjectQuery {
    ReplicateProject { project: message_board::ProjectId },
    LocalCancelDelivery { cancel: ItemId },
    DrainProjectPublisher { project: message_board::ProjectId },
    SeededHandoffFrontier { project: message_board::ProjectId },
}

pub async fn serve_until_shutdown(runtime: &mut CollaborationRuntime) -> io::Result<()> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut input_open = true;
    let mut output = tokio::io::stdout();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        let line = tokio::select! {
            result = tokio::signal::ctrl_c() => return result,
            _ = terminate.recv() => return Ok(()),
            error = runtime.listener_failure() => return Err(io::Error::other(error)),
            line = input.next_line(), if input_open => line?,
        };
        let Some(line) = line else {
            input_open = false;
            continue;
        };
        let query: ProjectQuery = serde_json::from_str(&line).map_err(io::Error::other)?;
        let reply = match query {
            ProjectQuery::ReplicateProject { project } => {
                runtime.runtime_test_replicate_project(&project).await?;
                serde_json::json!({"kind": "projectReplicated", "project": project})
            }
            ProjectQuery::LocalCancelDelivery { cancel } => {
                let recorded = runtime
                    .runtime_test_has_local_cancel_delivery(cancel)
                    .await?;
                serde_json::json!({"kind": "localCancelDelivery", "cancel": cancel, "recorded": recorded})
            }
            ProjectQuery::DrainProjectPublisher { project } => {
                runtime
                    .runtime_test_drain_project_publisher(&project)
                    .await?;
                serde_json::json!({"kind":"projectPublisherIdle", "project":project})
            }
            ProjectQuery::SeededHandoffFrontier { project } => {
                let admission = runtime
                    .runtime_test_project_admission()
                    .ok_or_else(|| io::Error::other("test project admission is unavailable"))?;
                let context = admission.runtime_test_context();
                let store = context
                    .registry
                    .existing_project(&project)
                    .await
                    .map_err(io::Error::other)?
                    .ok_or_else(|| io::Error::other("test project is absent"))?;
                let seeded_through = store
                    .verified_seed_frontier()
                    .await
                    .map_err(io::Error::other)?
                    .ok_or_else(|| {
                        io::Error::other("test project has no verified seed frontier")
                    })?;
                let mut read = store.snapshot().await.map_err(io::Error::other)?;
                let head = read.mail_handoff_head().await.map_err(io::Error::other)?;
                let live_through = read
                    .mail_settled_through()
                    .await
                    .map_err(io::Error::other)?;
                read.finish().await.map_err(io::Error::other)?;
                serde_json::json!({"kind":"seededHandoffFrontier", "project":project, "frontier":{
                    "seededThrough":seeded_through, "head":head, "liveThrough":live_through,
                }})
            }
        };
        let mut reply = serde_json::to_vec(&reply).map_err(io::Error::other)?;
        reply.push(b'\n');
        output.write_all(&reply).await?;
        output.flush().await?;
    }
}
