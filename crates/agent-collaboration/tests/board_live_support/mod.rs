//! Real two-agent board collaboration through the public CLI and SDK.
use message_board::ReadActor;
mod board_cli_runner;
mod expanded_cli_journey;
mod filtered_search_journey;
mod persistence_verification;
use crate::proof_context::{ProofContext, ProofResult, shell_quote};
use collaboration_client::board::{
    ActingForIdentity, BoardCreateRequest, BoardId, Description, HumanId, Identity, PageRequest,
    Placement, PostListRequest, PostListScope, PostReferences, PostSelection, PostShowRequest,
    ProjectCreateRequest, ProjectId, ReferenceTarget, ResourceName, SessionRef as BoardSessionRef,
    ThreadShowRequest, WorkspaceCreateRequest, WorkspaceId,
};
use collaboration_client::protocol::{
    MessageContent, MessageDelivery, SessionMessageSendParams, SessionRef,
};
use serde_json::{Value, json};

pub async fn exercise() -> ProofResult<()> {
    let mut proof = ProofContext::connect().await?;
    let alpha = proof.start_thread("Luna board researcher").await?;
    let beta = proof.start_thread("Luna board verifier").await?;
    let alpha_identity = board_identity(&alpha)?;
    let beta_identity = board_identity(&beta)?;
    let owner = ActingForIdentity::Human {
        human_id: HumanId::try_from("board-debug-acceptance-owner".to_owned())?,
    };

    let workspace_id = WorkspaceId::generate();
    let board_id = BoardId::generate();
    let project_id = ProjectId::generate();
    let thread_id = collaboration_client::board::PostId::generate();
    let contribution_message_id = collaboration_client::board::PostId::generate();
    let marker_id = thread_id.as_str().replace('-', "");
    let finding_marker = format!("BOARD_FINDING_{marker_id}");
    let result_marker = format!("BOARD_RESULT_{marker_id}");
    let alpha_posted_marker = format!("ALPHA_POSTED_{marker_id}");
    let beta_posted_marker = format!("BETA_POSTED_{marker_id}");
    let alpha_read_marker = format!("ALPHA_READ_{marker_id}");

    let project = proof
        .client
        .workspace_create(WorkspaceCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            resubmits: None,
            workspace_id: workspace_id.clone(),
            name: ResourceName::try_from(format!("Debug project {marker_id}"))?,
            description: Description::try_from("Disposable live board acceptance".to_owned())?,
            actor: alpha_identity.clone(),
            acting_for: Some(owner.clone()),
        })
        .await?;
    let board = proof
        .client
        .board_create(BoardCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            resubmits: None,
            kind: message_board::BoardKind::Area {},
            board_id: board_id.clone(),
            workspace_id: workspace_id.clone(),
            name: ResourceName::try_from(format!("Debug board {marker_id}"))?,
            description: Description::try_from("Two-agent CLI collaboration".to_owned())?,
            actor: alpha_identity.clone(),
            acting_for: Some(owner.clone()),
        })
        .await?;
    let topic = proof
        .client
        .project_create(ProjectCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            resubmits: None,
            project_id: project_id.clone(),
            board_id: board_id.clone(),
            title: ResourceName::try_from("Verification".to_owned())?,
            actor: alpha_identity.clone(),
            acting_for: Some(owner),
        })
        .await?;
    proof.record(
        "boardResourcesCreated",
        json!({"workspace":project,"board":board,"project":topic}),
    )?;

    let discussion_stream = arrange_discussion_subject(&mut proof, &project_id).await?;
    let cli = shell_quote(env!("CARGO_BIN_EXE_agent-collaboration"));
    let service_directory = shell_quote(&proof.service_directory.to_string_lossy());
    let alpha_actor = shell_quote(&serde_json::to_string(&alpha_identity)?);
    let beta_actor = shell_quote(&serde_json::to_string(&beta_identity)?);
    let finding_text = format!(
        "{finding_marker}: take the integer 37, multiply it by 3, then add 5. Post the decimal result in this root thread."
    );
    let finding_file = proof.workspace.join("board-finding.txt");
    std::fs::write(&finding_file, &finding_text)?;
    let alpha_post_command = format!(
        "{cli} thread create --post-id {} --project-id {} --about workstream:{} --role orchestrator --no-watch --text-file {} --actor {alpha_actor} --service-directory {service_directory} --json",
        thread_id.as_str(),
        project_id.as_str(),
        discussion_stream.as_str(),
        shell_quote(&finding_file.to_string_lossy()),
    );
    send_task(
        &mut proof,
        &alpha,
        &format!(
            "Execute this exact CLI command once:\n{alpha_post_command}\nIf it succeeds, output exactly {alpha_posted_marker}. If it fails, report the exact error and do not retry, resend, or use another communication path."
        ),
        "alphaBoardTaskAccepted",
    )
    .await?;
    let alpha_post_turns = proof.wait_for_text(&alpha, &alpha_posted_marker).await?;
    require_successful_cli_output(&alpha_post_turns, thread_id.as_str())?;

    let beta_read_command = format!(
        "{cli} post show --post-id {} --service-directory {service_directory} --json",
        thread_id.as_str(),
    );
    let beta_post_prefix = format!(
        "{cli} post post --post-id {} --placement thread --thread-id {} --reference-post {} --actor {beta_actor} --service-directory {service_directory} --json --text",
        contribution_message_id.as_str(),
        thread_id.as_str(),
        thread_id.as_str(),
    );
    send_task(
        &mut proof,
        &beta,
        &format!(
            "Run this read-only CLI command exactly once and inspect its returned message text:\n{beta_read_command}\nThe text contains an arithmetic instruction. Compute it yourself. Then run one CLI post command beginning exactly with:\n{beta_post_prefix}\nSupply one final quoted text argument whose entire value is `{result_marker}: ` followed by only the decimal result. Do not post unless the read command showed {finding_marker}. After the post succeeds, output exactly {beta_posted_marker}. If either command fails, report its exact error and do not retry or resend."
        ),
        "betaBoardTaskAccepted",
    )
    .await?;
    let beta_turns = proof.wait_for_text(&beta, &beta_posted_marker).await?;
    require_successful_cli_output(&beta_turns, &finding_marker)?;
    require_successful_cli_output(&beta_turns, contribution_message_id.as_str())?;

    let alpha_read_command = format!(
        "{cli} post list --scope thread --thread-id {} --selection latest --service-directory {service_directory} --json",
        thread_id.as_str(),
    );
    send_task(
        &mut proof,
        &alpha,
        &format!(
            "Execute this exact read-only CLI command once:\n{alpha_read_command}\nOnly if its result contains `{result_marker}: 116`, output exactly {alpha_read_marker}. Otherwise report what the command returned. Do not retry."
        ),
        "alphaThreadReadTaskAccepted",
    )
    .await?;
    let alpha_read_turns = proof.wait_for_text(&alpha, &alpha_read_marker).await?;
    require_successful_cli_output(&alpha_read_turns, &format!("{result_marker}: 116"))?;

    let root = proof
        .client
        .post_show(PostShowRequest {
            post_id: thread_id.clone(),
        })
        .await?;
    let contribution = proof
        .client
        .post_show(PostShowRequest {
            post_id: contribution_message_id.clone(),
        })
        .await?;
    let thread_history = proof
        .client
        .post_list(PostListRequest {
            scope: PostListScope::Thread {
                thread: message_board::ThreadId::from(thread_id.clone()).into(),
            },
            selection: PostSelection::Latest,
            page: PageRequest::default(),
        })
        .await?;
    let alpha_thread = proof
        .client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: message_board::ThreadId::from(thread_id.clone()).into(),
            reader: ReadActor::Identified {
                reader: alpha_identity.clone(),
            },
        })
        .await?;
    let beta_thread = proof
        .client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: message_board::ThreadId::from(thread_id.clone()).into(),
            reader: ReadActor::Identified {
                reader: beta_identity.clone(),
            },
        })
        .await?;

    verify_sdk_observations(
        &root.0,
        &contribution.0,
        &thread_history.page.records,
        &alpha_identity,
        &beta_identity,
        &finding_text,
        &format!("{result_marker}: 116"),
        &thread_id,
    )?;
    if alpha_thread
        .watch_status
        .as_ref()
        .is_none_or(|status| !status.watching)
        || beta_thread
            .watch_status
            .as_ref()
            .is_none_or(|status| !status.watching)
    {
        return Err("automatic thread watches were not visible for both posting agents".into());
    }
    expanded_cli_journey::exercise_after_agent_exchange(
        &mut proof,
        expanded_cli_journey::AgentExchange {
            alpha_identity: &alpha_identity,
            beta_identity: &beta_identity,
            first_workspace_id: workspace_id.as_str(),
            first_board_id: board_id.as_str(),
            first_project_id: project_id.as_str(),
            first_subject: &format!("workstream:{}", discussion_stream.as_str()),
            root_message: &root.0,
            contribution_message: &contribution.0,
        },
    )
    .await?;
    proof.record(
        "boardAgentExchangeVerified",
        json!({
            "root":root,
            "contribution":contribution,
            "threadHistory":thread_history,
            "alphaThread":alpha_thread,
            "betaThread":beta_thread,
        }),
    )?;
    Ok(())
}

pub async fn verify_persistence_after_host_restart() -> ProofResult<()> {
    persistence_verification::verify().await
}

async fn send_task(
    proof: &mut ProofContext,
    target: &SessionRef,
    task: &str,
    event: &str,
) -> ProofResult<()> {
    let receipt = proof
        .client
        .send_agent_message(SessionMessageSendParams {
            item_id: None,
            target: target.clone(),
            generation_guard: Some(proof.generation.clone()),
            message: MessageContent::Agent {
                sender: target.clone(),
                text: task.to_owned().try_into()?,
            },
            mode: MessageDelivery::Auto,
        })
        .await?;
    proof.record(event, json!(receipt))?;
    Ok(())
}

fn board_identity(session: &SessionRef) -> ProofResult<Identity> {
    let session: BoardSessionRef = serde_json::from_value(serde_json::to_value(session)?)?;
    Ok(Identity::Session { session })
}

fn require_successful_cli_output(turns: &[Value], marker: &str) -> ProofResult<()> {
    let observed = turns
        .iter()
        .filter_map(|turn| turn.get("items").and_then(Value::as_array))
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("commandExecution"))
        .filter(|item| item.get("exitCode").and_then(Value::as_i64) == Some(0))
        .filter_map(|item| item.get("aggregatedOutput").and_then(Value::as_str))
        .any(|output| output.contains(marker));
    if !observed {
        return Err(format!(
            "agent emitted its completion marker without successful CLI output containing {marker}"
        )
        .into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn verify_sdk_observations(
    root: &collaboration_client::board::Post,
    contribution: &collaboration_client::board::Post,
    thread_records: &[collaboration_client::board::Post],
    alpha_identity: &Identity,
    beta_identity: &Identity,
    finding_text: &str,
    result_text: &str,
    thread_id: &collaboration_client::board::PostId,
) -> ProofResult<()> {
    if root.actor != *alpha_identity
        || root.text.as_str() != finding_text
        || contribution.actor != *beta_identity
        || contribution.text.as_str() != result_text
        || contribution.placement
            != (Placement::Thread {
                thread_id: thread_id.clone().into(),
            })
        || contribution.references
            != PostReferences::try_from(vec![ReferenceTarget::Post {
                post_id: thread_id.clone(),
            }])?
        || thread_records.len() != 1
        || thread_records.first() != Some(contribution)
    {
        return Err(
            "public SDK reads did not reproduce the two agents' exact board exchange".into(),
        );
    }
    Ok(())
}

// Explicit human authority setup belongs to this ignored, isolated live fixture.
async fn arrange_discussion_subject(
    proof: &mut ProofContext,
    project_id: &ProjectId,
) -> ProofResult<message_board::WorkstreamId> {
    let discussion_stream = message_board::WorkstreamId::generate();
    let fixture_owner = json!({"kind":"human","humanId":"board-debug-acceptance-owner"});
    let owner_request = |operation: Value| json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,"resubmits":null,"observedEpoch":1,"actor":fixture_owner,"actingFor":null,"projectId":project_id,"operation":operation});
    let role = proof.client.role_grant(serde_json::from_value(owner_request(json!({"kind":"roleGrant","subject":{"kind":"project","subject":project_id},"role":"lead","holder":fixture_owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null})))?).await?;
    let stream = proof.client.workstream_create(serde_json::from_value(owner_request(json!({"kind":"workstreamCreate","workstreamId":discussion_stream,"name":"Live collaboration discussion","goal":"Explicit non-project conversation subject"})))?).await?;
    proof.record(
        &format!("explicitDiscussionSubject-{}", project_id.as_str()),
        json!({"role":role,"workstream":stream}),
    )?;

    Ok(discussion_stream)
}
