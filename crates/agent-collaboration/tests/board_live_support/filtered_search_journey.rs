use super::board_cli_runner::{result, run_board_cli, run_rejected_board_cli};
use crate::proof_context::{ProofContext, ProofResult};
use collaboration_client::board::PostId;
use serde_json::{Value, json};

pub(super) struct FilteredSearchProof<'a> {
    pub reader: &'a str,
    pub selected_workspace_id: &'a str,
    pub selected_board_id: &'a str,
    pub selected_project_id: &'a str,
    pub selected_subject: &'a str,
    pub selected_message_id: &'a str,
    pub watched_root_id: &'a str,
    pub watched_workspace_id: &'a str,
    pub thread_text: &'a str,
}

pub(super) async fn exercise(
    proof: &mut ProofContext,
    context: FilteredSearchProof<'_>,
) -> ProofResult<()> {
    let cooldown_text = proof.workspace.join("cooldown-refused-root.md");
    std::fs::write(&cooldown_text, "must use a thread during cooldown")?;
    let cooldown = run_rejected_board_cli(
        &proof.service_directory,
        "postCooldown",
        [
            "thread",
            "create",
            "--post-id",
            PostId::generate().as_str(),
            "--project-id",
            context.selected_project_id,
            "--about",
            context.selected_subject,
            "--role",
            "orchestrator",
            "--no-watch",
            "--actor",
            context.reader,
            "--text-file",
            cooldown_text.to_str().ok_or("cooldown text path")?,
        ],
    )
    .await?;
    if !cooldown
        .pointer("/result/record/result/decision/reasons/0/retryAfterSeconds")
        .and_then(Value::as_u64)
        .is_some_and(|seconds| (1..=60).contains(&seconds))
    {
        return Err("live cooldown reply omitted its bounded remaining time".into());
    }
    let discovery = run_board_cli(
        &proof.service_directory,
        [
            "search",
            "--query",
            "cross",
            "--scope",
            "workspace",
            "--workspace-id",
            context.selected_workspace_id,
            "--kind",
            "project",
        ],
    )
    .await?;
    if !result(&discovery, "/page/records")?
        .as_array()
        .is_some_and(|records| {
            records.iter().any(|record| {
                record.pointer("/project/id").and_then(Value::as_str)
                    == Some(context.selected_project_id)
            })
        })
    {
        return Err("live discovery omitted selected topic".into());
    }
    let latest = run_board_cli(
        &proof.service_directory,
        [
            "inbox",
            "fetch",
            "--scope",
            "project",
            "--project-id",
            context.selected_project_id,
            "--read-mode",
            "latest",
            "--reader",
            context.reader,
        ],
    )
    .await?;
    let records = result(&latest, "/page/records")?
        .as_array()
        .ok_or("latest records missing")?;
    if !records.iter().any(|record| {
        record.pointer("/post/postId").and_then(Value::as_str) == Some(context.selected_message_id)
    }) || !records.iter().any(|record| {
        record
            .pointer("/post/placement/threadId")
            .and_then(Value::as_str)
            == Some(context.watched_root_id)
    }) {
        return Err(
            "live filtered latest lost selected top-level or outside watched thread".into(),
        );
    }
    let unread = run_board_cli(
        &proof.service_directory,
        [
            "inbox",
            "fetch",
            "--scope",
            "board",
            "--board-id",
            context.selected_board_id,
            "--read-mode",
            "unread",
            "--reader",
            context.reader,
        ],
    )
    .await?;
    if !result(&unread, "/page/records")?
        .as_array()
        .is_some_and(|records| {
            records.iter().any(|record| {
                record.get("kind").and_then(Value::as_str) == Some("threadStateChanged")
                    && record.get("threadId").and_then(Value::as_str)
                        == Some(context.watched_root_id)
                    && record.get("workspaceId").and_then(Value::as_str)
                        == Some(context.watched_workspace_id)
            })
        })
    {
        return Err(
            "live filtered unread lost out-of-project state activity or its location".into(),
        );
    }
    let search = run_board_cli(
        &proof.service_directory,
        [
            "post",
            "search",
            "--query",
            context.thread_text,
            "--scope",
            "thread",
            "--thread-id",
            context.watched_root_id,
            "--kind",
            "thread",
        ],
    )
    .await?;
    if !result(&search, "/page/records")?
        .as_array()
        .is_some_and(|records| {
            records.iter().any(|record| {
                record.pointer("/post/text").and_then(Value::as_str) == Some(context.thread_text)
            })
        })
    {
        return Err("live message search omitted actual thread text".into());
    }
    let strict_search = run_board_cli(
        &proof.service_directory,
        [
            "post",
            "search",
            "--query",
            context.thread_text,
            "--scope",
            "project",
            "--project-id",
            context.selected_project_id,
        ],
    )
    .await?;
    if !result(&strict_search, "/page/records")?
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        return Err("live search incorrectly included out-of-scope watched content".into());
    }
    proof.record("filteredInboxAndSearchCliVerified",json!({
        "discovery":discovery,"latest":latest,"unread":unread,"search":search,"strictSearch":strict_search,"cooldown":cooldown
    }))?;
    Ok(())
}
