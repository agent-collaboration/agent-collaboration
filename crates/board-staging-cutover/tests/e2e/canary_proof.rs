//! An idempotent public canary uses the recovered head, then reads its actual record.
use super::{
    cutover_host_fixture::CutoverHostFixture, host_process::OwnedHost,
    intent_recovery_proof::RecoveredIntent, offline_cli_fixture::TestResult,
    recovery_rows::require,
};
use board_staging_cutover::PreparedProjectBundle;
use collaboration_client::CollaborationClient;
use message_board::public_replies::{RequestDecision, RequestResult};
use message_board::{
    BoardOperation, FeedCursor, FeedRead, Identity, IntentState, NotifyIntent, PageLimit,
    ProjectRecordShowRequest, ProjectRequest, ProtocolVersion, ReadActor, Record,
    WorkTrailReadResult, WorkTrailSelection, WorkTrailShowRequest,
};

pub(super) async fn append_and_read(
    host_root: &CutoverHostFixture,
    bundle: &PreparedProjectBundle,
    before: &NotifyIntent,
    recovered: RecoveredIntent,
    operator: Option<&super::offline_cli_fixture::RunFixture>,
) -> TestResult {
    let host = OwnedHost::start(&host_root.root, &bundle.manifest().header.declared_owner).await?;
    let observed = async {
        host.await_ready().await?;
        let directory = codex_router_host::router_collaboration_directory(&host_root.root);
        let client = CollaborationClient::connect(&directory,"cutover-canary","1").await?;
        let answer = super::public_claim::claim(&client,bundle).await?;
        require(matches!(answer,board_sync_protocol::ProjectClaimReply::Claimed { epoch, .. } if epoch.value()==2),"reopened public Claim did not retain e2")?;
        let project = &bundle.manifest().source.selected.project;
        let intent_request = ProjectRecordShowRequest { project_id:project.clone(),record:before.intent_id.clone().into() };
        let Record::NotifyIntent(reopened) = client.intent_show(intent_request.clone()).await? else { return Err("recovered intent disappeared after restart".into()); };
        require(reopened.state()==&IntentState::HandedOff { item:recovered.item.clone() },"restart changed the recovered handoff identity")?;
        let read = client.feed_read(message_board::FeedReadRequest { reader:ReadActor::Anonymous {},project_id:project.clone(),after:recovered.head,limit:PageLimit::try_from(1)?,include_redacted:true }).await?;
        let FeedRead::Entries { high_water:head, .. } = read.value else { return Err("recovered head is no longer a valid cursor".into()); };
        require(head==recovered.head,"restart appended unexplained application history")?;
        let row_id:message_board::WorkTrailRowId = bundle.manifest().canary_request.as_str().to_owned().try_into()?;
        let text:message_board::TrailText = format!("C5b cutover canary {}",bundle.manifest().run_id).try_into()?;
        let request = ProjectRequest {
            request_id:bundle.manifest().canary_request.clone(),project_id:project.clone(),
            actor:Identity::Human { human_id:bundle.manifest().header.declared_owner.clone() },acting_for:None,protocol_version:ProtocolVersion::CURRENT,
            operation:BoardOperation::WorkTrailAdd { row_id:row_id.clone(),subject:Some(message_board::TrailSubject::Project),text:text.clone(),tags:vec![],links:vec![] },
        };
        if let Some(run) = operator {
            super::compiled_canary_proof::verify(&client, bundle, run, head).await?;
        }
        let written = client.work_trail_add(request.clone()).await?;
        if !matches!(written.result,RequestResult::Decided(RequestDecision::Accepted { .. })) {
            std::fs::write(host_root.root.join("canary-outcome.json"),serde_json::to_vec(&written.result)?)?;
            return Err("public canary was not accepted".into());
        }
        let replayed = client.work_trail_add(request).await?;
        require(replayed.result==written.result,"frozen canary replay changed its outcome")?;
        let shown = client.work_trail_show(WorkTrailShowRequest { reader:ReadActor::Anonymous {},project_id:project.clone(),selection:WorkTrailSelection::Forward { after:head },limit:PageLimit::try_from(10)?,subject:None,tags:vec![],include_redacted:true }).await?;
        let WorkTrailReadResult::Forward(page) = shown.value else { return Err("canary readback did not retain its recovered cursor".into()); };
        let [row] = page.rows.as_slice() else { return Err("canary replay lost or duplicated its trail record".into()); };
        let expected = FeedCursor { epoch:head.epoch,position:head.position.get().checked_add(1).ok_or("canary cursor overflow")?.try_into()? };
        require(row.row_id==row_id && row.text==text && row.envelope.request.as_ref()==Some(&bundle.manifest().canary_request) && row.envelope.position.project_cursor()==Some(expected),"canary did not append at recovered head+1 and read back intact")?;
        let Record::NotifyIntent(after_canary) = client.intent_show(intent_request).await? else { return Err("canary lost the recovered intent".into()); };
        require(after_canary==reopened,"canary or restart repeated the intent progression")?;
        drop(client);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let stopped = host.stop().await;
    observed?;
    stopped?;
    Ok(())
}
