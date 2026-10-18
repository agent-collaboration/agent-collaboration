//! Release-only measurements share the realistic fixture and public MCP admission.
use super::board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::json;
use std::time::Instant;

pub async fn measure_remaining_families(
    fixture: &BoardFixture,
    project: &ProjectId,
    workstream: &WorkstreamId,
    task: &TaskId,
    thread: &ThreadId,
) -> Result<(), TestError> {
    for family in [
        "workstream",
        "subtask",
        "trail",
        "role",
        "process",
        "membership",
        "fleet",
    ] {
        let mut samples = Vec::new();
        for sample in 0..50 {
            let (tool, request) = match family {
                "workstream" => ("workstream_rename", fixture.request(project, json!({
                    "kind":"workstreamRename","workstreamId":workstream,"name":format!("measured workstream {sample}"),"expectedRevision":sample
                }))),
                "subtask" => ("subtask_add", fixture.request(project, json!({
                    "kind":"subtaskAdd","subtaskId":SubtaskId::generate(),"taskId":task,"text":"measured subtask","place":format!("{:02x}",sample+1)
                }))),
                "trail" => ("work_trail_add", fixture.request(project, json!({
                    "kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":"measured trail","tags":[],"links":[]
                }))),
                "role" => ("role_grant", fixture.request(project, json!({
                    "kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer",
                    "holder":{"kind":"human","humanId":format!("measured holder {sample}")},"model":null,"lineage":null,"expectedRevision":sample,"ownerGrant":null
                }))),
                "process" => ("project_publish_plan", fixture.request(project, json!({
                    "kind":"projectPublishPlan","hash":format!("{sample:064x}"),"document":{"kind":"url","url":"https://example.com/measured-plan"},"expectedRevision":sample
                }))),
                "membership" => {
                    let mut request = fixture.header();
                    request.as_object_mut().ok_or("header")?.extend(json!({"thread":{"thread":thread},"observedEpoch":1}).as_object().ok_or("fields")?.clone());
                    (if sample % 2 == 0 { "thread_watch" } else { "thread_unwatch" }, request)
                }
                "fleet" => {
                    let mut request = fixture.header();
                    request.as_object_mut().ok_or("header")?.extend(json!({"workspaceId":fixture.workspace,"name":format!("measured workspace {sample}"),"description":"measured fleet update"}).as_object().ok_or("fields")?.clone());
                    ("workspace_update", request)
                }
                _ => return Err("unknown measured family".into()),
            };
            let started = Instant::now();
            let reply = fixture.api.call(tool, request).await?;
            samples.push(started.elapsed().as_micros());
            accepted(&reply);
        }
        samples.sort_unstable();
        let p95 = samples.get(47).ok_or("p95")?;
        eprintln!("write family={family} samples=50 p95_us={p95}");
    }
    Ok(())
}
