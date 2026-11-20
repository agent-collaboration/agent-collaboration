//! Real Codex app-server proofs for held DM and subscription push cells.
use super::{OBSERVATION_TIMEOUT, ProofContext, ProofResult, dm_notice_and_show, matrix_marker};
use codex_native_integration::NativeOperation;
use collaboration_client::protocol::{PushRecordShowParams, RouterAccess, SessionRef};
use collaboration_client::{
    ConversationClient, ConversationLoadInput, ConversationOperationResult,
};
use collaboration_protocol::PushDeliveryState;
use serde_json::{Value, json};
use std::time::Duration;

pub(super) struct UnloadedMatrixTargets {
    pub(super) held_dm: SessionRef,
}

pub(super) async fn prepare_unloaded_targets(
    proof: &mut ProofContext,
) -> ProofResult<UnloadedMatrixTargets> {
    let targets = UnloadedMatrixTargets {
        held_dm: prepare_materialized_target(proof, "S3 held DM recipient").await?,
    };
    super::debug_backend_restart::restart(proof).await?;
    for target in [&targets.held_dm] {
        require_unloaded_target(proof, target).await?;
    }
    proof.record(
        "matrixUnloadedTargetsPrepared",
        json!({
            "heldDm":targets.held_dm,
            "generation":proof.generation,
        }),
    )?;
    Ok(targets)
}

async fn prepare_materialized_target(
    proof: &mut ProofContext,
    role: &str,
) -> ProofResult<SessionRef> {
    let target = proof.start_thread(role).await?;
    let marker = matrix_marker("unloaded-preparation");
    let task = format!("Output exactly {marker}. Do not call tools or spawn agents.");
    let response = proof
        .native
        .request_validated(
            &proof.schemas,
            NativeOperation::StartTurn,
            json!({"threadId":String::from(target.session_id.clone()),
            "input":[{"type":"text","text":task,"text_elements":[]}]}),
        )
        .await?;
    let turn_id = response
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .ok_or("Unloaded preparation did not accept a native turn")?
        .to_owned();
    let deadline = tokio::time::Instant::now() + OBSERVATION_TIMEOUT;
    let thread_id = String::from(target.session_id.clone());
    // Acknowledging turn/start precedes persistence of the first rollout. Observe
    // the accepted turn's completion before asking history to prove its contents.
    tokio::time::timeout_at(deadline, async {
        loop {
            let message = proof.native.next_message().await?;
            if message.get("id").is_some() && message.get("method").is_some() {
                return Err::<(), Box<dyn std::error::Error + Send + Sync>>(
                    "Preparation encountered an unexpected native callback".into(),
                );
            }
            if message.get("method").and_then(Value::as_str) == Some("turn/completed")
                && message.pointer("/params/threadId").and_then(Value::as_str)
                    == Some(thread_id.as_str())
                && message.pointer("/params/turn/id").and_then(Value::as_str)
                    == Some(turn_id.as_str())
            {
                if message
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str)
                    != Some("completed")
                {
                    return Err("Preparation turn did not complete successfully".into());
                }
                proof.record(
                    "preparationTurnCompleted",
                    json!({"target":target,"turnId":turn_id}),
                )?;
                return Ok(());
            }
        }
    })
    .await??;
    let mut observation = tokio::time::interval(Duration::from_millis(250));
    loop {
        observation.tick().await;
        let turns = proof.turns(&target).await?;
        if let Some(turn) = turns
            .iter()
            .find(|turn| turn.get("id").and_then(Value::as_str) == Some(turn_id.as_str()))
        {
            match turn.get("status").and_then(Value::as_str) {
                Some("completed") => {
                    let input_count = turn
                        .get("items")
                        .and_then(Value::as_array)
                        .ok_or("Completed preparation turn omitted its history")?
                        .iter()
                        .filter(|item| {
                            super::user_message_text(item).is_ok_and(|text| text == task)
                        })
                        .count();
                    if input_count != 1 || !super::proof_context::agent_text(turn).contains(&marker)
                    {
                        return Err("Preparation completed without the exact native input and requested output".into());
                    }
                    proof.record("matrixTargetMaterialized", json!({"role":role,"target":target,"turnId":turn_id,"input":task,"status":"completed"}))?;
                    return Ok(target);
                }
                Some("failed" | "interrupted") => {
                    proof.record(
                        "matrixTargetPreparationFailed",
                        json!({"role":role,"target":target,"turn":turn}),
                    )?;
                    return Err(format!("{role} preparation did not complete; model/auth/runtime failure is a failed cell").into());
                }
                _ => {}
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!(
                "{role} preparation did not complete within its deadline; cell is unverified"
            )
            .into());
        }
    }
}

async fn require_unloaded_target(proof: &mut ProofContext, target: &SessionRef) -> ProofResult<()> {
    let response = proof
        .native
        .request_validated(
            &proof.schemas,
            NativeOperation::ListLoadedThreads,
            json!({}),
        )
        .await?;
    if response
        .get("nextCursor")
        .is_some_and(|cursor| !cursor.is_null())
    {
        return Err("Unexpected pagination in private loaded-thread inventory".into());
    }
    let loaded = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or("Private loaded-thread inventory omitted its records")?;
    let target_id = String::from(target.session_id.clone());
    if loaded
        .iter()
        .any(|id| id.as_str() == Some(target_id.as_str()))
    {
        return Err(
            format!("Saved target {target_id} is still loaded; held cell not attempted").into(),
        );
    }
    Ok(())
}

pub(super) async fn cover_held_dm_cell(
    proof: &mut ProofContext,
    sender: &SessionRef,
    target: &SessionRef,
) -> ProofResult<()> {
    require_unloaded_target(proof, target).await?;
    let marker = matrix_marker("held-dm");
    let body = format!("{marker} delivered after conversation load");
    let receipt = super::cli_send(proof, Some(sender), target, &body, false).await?;
    let push_id = super::receipt_push_id(&receipt)?;
    if receipt.get("deliveryState") != Some(&json!("held")) {
        return Err(format!("S3 send was not held: {receipt}").into());
    }

    let (held, expected_line) = dm_notice_and_show(proof, target, &push_id).await?;
    if held.record.delivery_state != PushDeliveryState::Held {
        return Err("S3 show did not preserve the held DM state".into());
    }

    load_codex_target(proof, sender, target).await?;
    let item = super::wait_for_codex_input(proof, target, &marker, OBSERVATION_TIMEOUT).await?;
    let observed_line = super::user_message_text(&item)?;
    super::assert_exact_notice_line(&observed_line, &expected_line)?;
    // The first show marked this DM read, so it no longer belongs in the unread inbox.
    let delivered = proof
        .client
        .router_show(PushRecordShowParams {
            caller: target.clone(),
            reference: held.link.clone(),
        })
        .await?;
    let collaboration_client::protocol::MessageShowResult::Local(delivered) = delivered else {
        return Err("native proof expected a stored local push".into());
    };
    let delivered_line = super::expected_push_line(&delivered, &super::machine_label(proof)?)?;
    if delivered.record.delivery_state != PushDeliveryState::Delivered
        || delivered.record.push_id.as_str() != push_id
        || delivered.record.target != *target
        || delivered_line != expected_line
    {
        return Err("S3 load did not deliver the original stored DM line".into());
    }
    proof.record(
        "pushDeliveryMatrixS3",
        json!({"target":target,"pushId":push_id,"link":delivered.link,"deliveryState":"delivered","line":observed_line}),
    )?;
    Ok(())
}

async fn load_codex_target(
    proof: &ProofContext,
    requested_by: &SessionRef,
    target: &SessionRef,
) -> ProofResult<()> {
    let conversation = ConversationClient::connect(
        &collaboration_client::CollaborationAccess::api(&proof.service_directory),
        &target.endpoint,
    )
    .await?;
    let result = conversation
        .load(
            ConversationLoadInput {
                operation_id: None,
                target: target.clone(),
                working_directory: proof.workspace.clone(),
                requested_by: requested_by.clone().into(),
                approver: Some(requested_by.clone().into()),
                access: RouterAccess::WorkspaceWrite,
                generation: None,
            },
            Duration::from_secs(90),
        )
        .await?;
    match result {
        ConversationOperationResult::Completed { target: loaded, .. } if loaded == *target => {
            Ok(())
        }
        other => Err(format!("Codex load returned an unexpected result: {other:?}").into()),
    }
}
