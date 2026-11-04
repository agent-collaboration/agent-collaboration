//! A separately enrolled actual Host reads the frozen canary from the real central copy.
use super::{
    cutover_host_fixture::CutoverHostFixture, host_process::OwnedHost,
    offline_cli_fixture::TestResult, recovery_rows::require,
};
use board_central_test_support::TestCentral;
use board_staging_cutover::PreparedProjectBundle;
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{
    ActorOrigin, BoardFailureKind, CopySource, CurrentWriter, FeedCursor, PageLimit,
    ProjectViewRequest, ReadActor, WorkTrailReadResult, WorkTrailSelection, WorkTrailShowRequest,
};
use std::{path::Path, time::Duration};

pub(super) async fn read_on_another_machine(
    parent: &Path,
    source_host: &CutoverHostFixture,
    central: &TestCentral,
    bundle: &PreparedProjectBundle,
    before_canary: FeedCursor,
) -> TestResult {
    // Arrange a distinct service/peer with no local writer or copied source directory.
    let reader_host = CutoverHostFixture::new_in(parent)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    require(
        reader_host.service != source_host.service && reader_host.peer != source_host.peer,
        "reader machine identity was reused",
    )?;
    let origin = ActorOrigin {
        installation: String::from(reader_host.service.clone()).try_into()?,
        machine: "cross-machine cutover reader".to_owned().try_into()?,
    };
    let enrollment = central
        .additional_machine(&origin, reader_host.peer)
        .await?;
    std::fs::create_dir_all(reader_host.root.join("fleet"))?;
    let document = enrollment.write_host_session(&reader_host.root)?;
    std::fs::rename(document, reader_host.root.join("fleet/enrollment.json"))?;
    let project = &bundle.manifest().source.selected.project;
    let reader_directory = codex_router_host::router_collaboration_directory(&reader_host.root);
    require(
        !reader_directory
            .join("projects")
            .join(project.as_str())
            .try_exists()?,
        "reader started with a project source",
    )?;
    let observed = async {
        let writer = OwnedHost::start(&source_host.root,&bundle.manifest().header.declared_owner).await?;
        let written = async {
            writer.await_ready().await?;
            let source_directory = codex_router_host::router_collaboration_directory(&source_host.root);
            let writer_client = CollaborationClient::connect(&source_directory,"cutover-copy-producer","1").await?;
            let claimed = super::public_claim::claim(&writer_client,bundle).await?;
            require(matches!(claimed,board_sync_protocol::ProjectClaimReply::Claimed {epoch,..} if epoch.value()==2),"producer did not retain its actual e2 writer")?;
            let reader = OwnedHost::start(&reader_host.root,&bundle.manifest().header.declared_owner).await?;
            let read = async {
                reader.await_ready().await?;
                let selected_directory = codex_router_host::router_collaboration_directory(&reader_host.root);
                let client = CollaborationClient::connect(&selected_directory,"cutover-copy-reader","1").await?;
                require(client.identity().service_id==reader_host.service,"canary read used the producer instead of the distinct enrolled Host")?;
                // Act through the new Host's public protocol; no native source is copied to it.
                read_frozen_canary(&client,bundle,before_canary).await?;
                // Assert this is the Central-backed read copy, while the producer keeps e2.
                let view = client.view_work(ProjectViewRequest {project_id:project.clone(),include_removed:true}).await?;
                require(matches!(view.source,CopySource::ReadCopy {}),"secondary read did not come from the central copy")?;
                require(matches!(view.current_writer,CurrentWriter::Held {epoch} if epoch.get()==2),"secondary read changed writer authority")?;
                require(!reader_directory.join("projects").join(project.as_str()).try_exists()?,"readback installed a second writer")?;
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
            }.await;
            let reader_stopped = reader.stop().await;
            read?;
            reader_stopped?;
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let writer_stopped = writer.stop().await;
        written?;
        writer_stopped?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let disconnected = enrollment.shutdown().await;
    if let Err(error) = observed {
        let retained = reader_host._area.keep();
        return Err(format!("{error}; owned reader retained at {}", retained.display()).into());
    }
    disconnected?;
    Ok(())
}

async fn read_frozen_canary(
    client: &CollaborationClient,
    bundle: &PreparedProjectBundle,
    before_canary: FeedCursor,
) -> TestResult {
    let expected_id: message_board::WorkTrailRowId = bundle
        .manifest()
        .canary_request
        .as_str()
        .to_owned()
        .try_into()?;
    let expected_text: message_board::TrailText =
        format!("C5b cutover canary {}", bundle.manifest().run_id).try_into()?;
    let expected_cursor = FeedCursor {
        epoch: before_canary.epoch,
        position: before_canary
            .position
            .get()
            .checked_add(1)
            .ok_or("canary cursor overflow")?
            .try_into()?,
    };
    tokio::time::timeout(Duration::from_secs(20),async {
        loop {
            let shown = client.work_trail_show(WorkTrailShowRequest {
                reader:ReadActor::Anonymous {},project_id:bundle.manifest().source.selected.project.clone(),
                selection:WorkTrailSelection::Forward {after:before_canary},limit:PageLimit::try_from(10)?,
                subject:None,tags:vec![],include_redacted:true,
            }).await;
            match shown {
                Ok(shown)=>{
                    let WorkTrailReadResult::Forward(page)=shown.value else {return Err("secondary copy lost the original recovered cursor".into());};
                    if page.rows.is_empty() {tokio::task::yield_now().await;continue;}
                    let [row]=page.rows.as_slice() else {return Err("secondary copy lost or duplicated the frozen canary".into());};
                    require(row.row_id==expected_id && row.text==expected_text
                        && row.envelope.request.as_ref()==Some(&bundle.manifest().canary_request)
                        && row.envelope.position.project_cursor()==Some(expected_cursor),"secondary copy changed the frozen canary body, request identity or recovered cursor")?;
                    return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());
                }
                Err(BoardClientError::Rejected(error)) if error.kind==BoardFailureKind::BoardUnavailable=>tokio::task::yield_now().await,
                Err(error)=>return Err(error.into()),
            }
        }
    }).await??;
    Ok(())
}
