use super::board_cli_runner::{
    require_record_message_ids, result, run_board_cli, run_rejected_board_cli,
};
use crate::proof_context::{ProofContext, ProofResult};
use collaboration_client::board::{Identity, Post, PostId};
use serde_json::{Value, json};
use std::{io::Write, os::unix::fs::OpenOptionsExt};

pub(super) struct AgentExchange<'a> {
    pub alpha_identity: &'a Identity,
    pub beta_identity: &'a Identity,
    pub first_workspace_id: &'a str,
    pub first_board_id: &'a str,
    pub first_project_id: &'a str,
    pub first_subject: &'a str,
    pub root_message: &'a Post,
    pub contribution_message: &'a Post,
}

pub(super) async fn exercise_after_agent_exchange(
    proof: &mut ProofContext,
    exchange: AgentExchange<'_>,
) -> ProofResult<()> {
    let AgentExchange {
        alpha_identity,
        beta_identity,
        first_workspace_id,
        first_board_id,
        first_project_id,
        first_subject,
        root_message,
        contribution_message,
    } = exchange;
    let alpha_actor = serde_json::to_string(alpha_identity)?;
    let beta_actor = serde_json::to_string(beta_identity)?;

    let alpha_inbox = run_board_cli(
        &proof.service_directory,
        [
            "inbox",
            "fetch",
            "--scope",
            "workspace",
            "--workspace-id",
            first_workspace_id,
            "--reader",
            &alpha_actor,
        ],
    )
    .await?;
    let contribution_cursor = match contribution_message.position {
        message_board::RecordPosition::Project(cursor) => cursor,
        message_board::RecordPosition::Fleet(_) | message_board::RecordPosition::Unsequenced => {
            return Err("post has no project position".into());
        }
    };
    let inbox_records = result(&alpha_inbox, "/page/records")?
        .as_array()
        .ok_or("alpha inbox records were not an array")?;
    if !inbox_records.iter().any(|record| {
        record.pointer("/post/postId").and_then(Value::as_str)
            == Some(contribution_message.post_id.as_str())
            && record.pointer("/position/position") == Some(&json!(contribution_cursor))
    }) {
        return Err("alpha inbox omitted beta's watched-thread contribution".into());
    }
    let entry = json!({"requestId":message_board::RequestId::generate(),"resubmits":null,"observedEpoch":contribution_cursor.epoch,"scope":{"kind":"thread","thread":{"thread":root_message.post_id}},"through":contribution_cursor}).to_string();
    run_board_cli(
        &proof.service_directory,
        [
            "inbox",
            "acknowledge",
            "--entry",
            &entry,
            "--actor",
            &alpha_actor,
        ],
    )
    .await?;
    let acknowledged_inbox = run_board_cli(
        &proof.service_directory,
        [
            "inbox",
            "fetch",
            "--scope",
            "workspace",
            "--workspace-id",
            first_workspace_id,
            "--reader",
            &alpha_actor,
        ],
    )
    .await?;
    if !result(&acknowledged_inbox, "/page/records")?
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        return Err("acknowledged alpha inbox still returned unread activity".into());
    }

    run_board_cli(
        &proof.service_directory,
        [
            "thread",
            "unwatch",
            "--thread-id",
            root_message.post_id.as_str(),
            "--actor",
            &beta_actor,
        ],
    )
    .await?;
    run_board_cli(
        &proof.service_directory,
        [
            "thread",
            "watch",
            "--thread-id",
            root_message.post_id.as_str(),
            "--actor",
            &beta_actor,
        ],
    )
    .await?;
    run_board_cli(
        &proof.service_directory,
        [
            "thread",
            "resolve",
            "--thread-id",
            root_message.post_id.as_str(),
            "--actor",
            &alpha_actor,
            "--reason",
            "live lifecycle proof",
        ],
    )
    .await?;
    run_rejected_board_cli(
        &proof.service_directory,
        "closed",
        [
            "post",
            "post",
            "--post-id",
            PostId::generate().as_str(),
            "--placement",
            "thread",
            "--thread-id",
            root_message.post_id.as_str(),
            "--text",
            "must be rejected while resolved",
            "--actor",
            &beta_actor,
        ],
    )
    .await?;
    run_board_cli(
        &proof.service_directory,
        [
            "thread",
            "unresolve",
            "--thread-id",
            root_message.post_id.as_str(),
            "--actor",
            &alpha_actor,
        ],
    )
    .await?;

    let second_workspace_id = collaboration_client::board::WorkspaceId::generate();
    let second_board_id = collaboration_client::board::BoardId::generate();
    let second_project_id = collaboration_client::board::ProjectId::generate();
    let cross_project_message_id = PostId::generate();
    run_board_cli(
        &proof.service_directory,
        [
            "workspace",
            "create",
            "--workspace-id",
            second_workspace_id.as_str(),
            "--name",
            "Live cross-project proof",
            "--description",
            "Disposable second project",
            "--actor",
            &beta_actor,
        ],
    )
    .await?;
    run_board_cli(
        &proof.service_directory,
        [
            "board",
            "create",
            "--board-id",
            second_board_id.as_str(),
            "--workspace-id",
            second_workspace_id.as_str(),
            "--name",
            "Cross-project board",
            "--actor",
            &beta_actor,
        ],
    )
    .await?;
    run_board_cli(
        &proof.service_directory,
        [
            "project",
            "create",
            "--project-id",
            second_project_id.as_str(),
            "--board-id",
            second_board_id.as_str(),
            "--title",
            "Cross references",
            "--actor",
            &beta_actor,
        ],
    )
    .await?;
    let cross_subject = super::arrange_discussion_subject(proof, &second_project_id).await?;
    let cross_about = format!("workstream:{}", cross_subject.as_str());
    let cross_text = proof.workspace.join("cross-project-root.md");
    std::fs::write(&cross_text, "Cross-project reference proof")?;
    let cross_project_post = run_board_cli(
        &proof.service_directory,
        [
            "thread",
            "create",
            "--post-id",
            cross_project_message_id.as_str(),
            "--project-id",
            second_project_id.as_str(),
            "--about",
            &cross_about,
            "--role",
            "orchestrator",
            "--no-watch",
            "--text-file",
            cross_text.to_str().ok_or("cross text path")?,
            "--reference-post",
            contribution_message.post_id.as_str(),
            "--reference-thread",
            root_message.post_id.as_str(),
            "--actor",
            &beta_actor,
        ],
    )
    .await?;
    let cross_post = proof
        .client
        .post_show(collaboration_client::board::PostShowRequest {
            post_id: cross_project_message_id.clone(),
        })
        .await?
        .0;
    let cross_project_cursor = match cross_post.position {
        message_board::RecordPosition::Project(cursor) => cursor,
        message_board::RecordPosition::Fleet(_) | message_board::RecordPosition::Unsequenced => {
            return Err("cross-project post has no feed cursor".into());
        }
    };
    if result(&cross_project_post, "/record/result/decision/kind")?.as_str() != Some("accepted") {
        return Err("cross-project post was not accepted".into());
    }
    super::filtered_search_journey::exercise(
        proof,
        super::filtered_search_journey::FilteredSearchProof {
            reader: &beta_actor,
            selected_workspace_id: second_workspace_id.as_str(),
            selected_board_id: second_board_id.as_str(),
            selected_project_id: second_project_id.as_str(),
            selected_subject: &cross_about,
            selected_message_id: cross_project_message_id.as_str(),
            watched_root_id: root_message.post_id.as_str(),
            watched_workspace_id: first_workspace_id,
            thread_text: contribution_message.text.as_str(),
        },
    )
    .await?;
    let project_inventory = run_board_cli(
        &proof.service_directory,
        ["workspace", "list", "--limit", "100"],
    )
    .await?;
    let project_records = result(&project_inventory, "/page/records")?
        .as_array()
        .ok_or("project inventory records were not an array")?;
    for expected_project_id in [first_workspace_id, second_workspace_id.as_str()] {
        if !project_records.iter().any(|project| {
            project.get("workspaceId").and_then(Value::as_str) == Some(expected_project_id)
        }) {
            return Err(format!("CLI project inventory omitted {expected_project_id}").into());
        }
    }
    let range_upper_value = json!({"kind":"aggregate","cursor":{"perProject":{first_project_id:contribution_cursor, second_project_id.as_str():cross_project_cursor}}});
    let range_upper = range_upper_value.to_string();
    let range_start = json!({"kind":"aggregate","cursor":{"perProject":{}}}).to_string();
    let all_project_history = run_board_cli(
        &proof.service_directory,
        [
            "post",
            "list",
            "--scope",
            "all-workspaces",
            "--selection",
            "range",
            "--from",
            &range_start,
            "--to",
            &range_upper,
        ],
    )
    .await?;
    require_record_message_ids(
        &all_project_history,
        &[
            root_message.post_id.as_str(),
            cross_project_message_id.as_str(),
        ],
    )?;

    run_board_cli(
        &proof.service_directory,
        [
            "board",
            "archive",
            "--board-id",
            first_board_id,
            "--actor",
            &alpha_actor,
        ],
    )
    .await?;
    let archive_text = proof.workspace.join("archive-refused-root.md");
    std::fs::write(&archive_text, "must be rejected after archive")?;
    run_rejected_board_cli(
        &proof.service_directory,
        "archived",
        [
            "thread",
            "create",
            "--post-id",
            PostId::generate().as_str(),
            "--project-id",
            first_project_id,
            "--about",
            first_subject,
            "--role",
            "orchestrator",
            "--no-watch",
            "--text-file",
            archive_text.to_str().ok_or("archive text path")?,
            "--actor",
            &alpha_actor,
        ],
    )
    .await?;

    let marker: Value =
        serde_json::from_slice(&std::fs::read(proof.root.join("debug-host-context.json"))?)?;
    let host_pid = marker
        .get("hostPid")
        .and_then(Value::as_u64)
        .filter(|pid| *pid > 0)
        .ok_or("debug Host marker omitted its PID")?;
    let persistence_state = json!({
        "hostPid": host_pid,
        "firstWorkspaceId": first_workspace_id,
        "firstBoardId": first_board_id,
        "firstWorkspaceId": first_project_id,
        "secondWorkspaceId": second_workspace_id,
        "secondBoardId": second_board_id,
        "secondWorkspaceId": second_project_id,
        "threadId": root_message.post_id,
        "contributionMessageId": contribution_message.post_id,
        "crossProjectMessageId": cross_project_message_id,
        "rangeUpper": range_upper_value,
        "alphaIdentity": alpha_identity,
        "betaIdentity": beta_identity,
    });
    let mut state_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(proof.root.join("board-persistence-state.json"))?;
    writeln!(state_file, "{persistence_state}")?;
    proof.record(
        "boardFullCliJourneyReadyForHostRestart",
        json!({
            "firstWorkspaceId":first_workspace_id,
            "secondWorkspaceId":second_workspace_id,
            "rangeUpper":range_upper_value,
        }),
    )?;
    Ok(())
}
