//! Read real rich records and replay their old request after the new writer is installed.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    host_process::OwnedHost,
    offline_cli_fixture::{RunFixture, TestResult},
    recovery_rows::require,
    rich_source_messages::{REPLY_POST, ROOT_POST, RichMessages},
    rich_source_records::{LARGE_PR_NUMBER, RichRecords, UPDATED_TASK_BODY},
};
use board_central_test_support::TestCentral;
use board_staging_cutover::PreparedProjectBundle;
use collaboration_client::CollaborationClient;
use message_board::{
    FactKind, PageLimit, PageRequest, PostShowRequest, ProjectRecordShowRequest,
    ProjectViewRequest, ReadActor, Record, ReferenceTarget, SubtaskListRequest, ThreadShowRequest,
    ThreadSubject,
};

pub(super) async fn verify(
    host_root: &CutoverHostFixture,
    central: &TestCentral,
    run: &RunFixture,
    bundle: &PreparedProjectBundle,
    records: &RichRecords,
    messages: &RichMessages,
) -> TestResult {
    let expected_root_post: message_board::PostText = ROOT_POST.to_owned().try_into()?;
    let expected_reply: message_board::PostText = REPLY_POST.to_owned().try_into()?;
    let host = OwnedHost::start(&host_root.root, &bundle.manifest().header.declared_owner).await?;
    let observed = async {
        host.await_ready().await?;
        run.renew_owner(central.base_url())?;
        require(run.execute_claim().await?.status.success(),"rich target frozen Claim replay failed")?;
        let directory = codex_router_host::router_collaboration_directory(&host_root.root);
        let client = CollaborationClient::connect(&directory,"rich-cutover-readback","1").await?;
        let task = client.task_show(ProjectRecordShowRequest {project_id:records.project.clone(),record:records.first_task.clone().into()}).await?;
        let Record::Task(task)=task else {return Err("rich task record missing".into());};
        require(task.body==UPDATED_TASK_BODY && task.workstream_id==records.workstream,"rich task body or relationship changed")?;
        let other = client.task_show(ProjectRecordShowRequest {project_id:records.project.clone(),record:records.second_task.clone().into()}).await?;
        require(matches!(other,Record::Task(task) if task.workstream_id==records.workstream),"dependent task relationship changed")?;
        let fact = client.fact_show(ProjectRecordShowRequest {project_id:records.project.clone(),record:records.fact.clone().into()}).await?;
        require(matches!(fact,Record::Fact(fact) if matches!(fact.kind,FactKind::PrFact {number,..} if number==LARGE_PR_NUMBER)),"fact integer above the JavaScript precision limit changed")?;
        let subtasks = client.subtask_list(SubtaskListRequest {project_id:records.project.clone(),page:PageRequest {limit:PageLimit::try_from(10)?,cursor:None},include_removed:true}).await?;
        require(subtasks.records.iter().any(|record|matches!(record,Record::Subtask(row) if row.subtask_id==records.subtask && row.task_id==records.first_task && row.checked)),"checked subtask changed")?;
        let thread = client.thread_show(ThreadShowRequest {thread:messages.thread.clone(),reader:ReadActor::Anonymous {},include_redacted:true}).await?;
        require(thread.subject.about==ThreadSubject::Workstream(records.workstream.clone()),"refiled thread subject changed")?;
        let root_post = client.post_show(PostShowRequest {post_id:messages.root_post.clone()}).await?;
        require(root_post.0.text==expected_root_post,"root post body changed")?;
        let reply = client.post_show(PostShowRequest {post_id:messages.reply.clone()}).await?;
        require(reply.0.text==expected_reply
            && reply.0.references.as_slice()==[ReferenceTarget::Post {post_id:messages.root_post.clone()}],"reply body or reference changed")?;
        let before = client.view_work(ProjectViewRequest {project_id:records.project.clone(),include_removed:true}).await?;
        let replayed = client.task_update(records.replay.clone()).await?;
        require(replayed.result==records.replay_result,"old request decision did not replay intact")?;
        let after = client.view_work(ProjectViewRequest {project_id:records.project.clone(),include_removed:true}).await?;
        require(before.as_of==after.as_of,"replaying the old task request appended duplicate history")?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let stopped = host.stop().await;
    observed?;
    stopped?;
    Ok(())
}
