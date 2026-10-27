use super::*;
use agent_automation::RunPhase;

#[tokio::test]
async fn known_rejected_provider_submission_after_retention_uses_fresh_id_and_old_link_expires()
-> TestResult<()> {
    let (root, store, worker, run_id, observed_push_ids) = provider_worker_fixture(
        FakeSubmissionPlan::RejectOnceThenAccept,
        FakeSettlementPlan::Pending,
    )
    .await?;
    Box::pin(worker.step(run_id.clone()))
        .await
        .map_err(|error| format!("initial target preparation failed: {error:?}"))?;
    Box::pin(worker.step(run_id.clone()))
        .await
        .map_err(|error| format!("initial busy submission failed: {error:?}"))?;
    Box::pin(worker.step(run_id.clone()))
        .await
        .map_err(|error| format!("known rejection persistence failed: {error:?}"))?;
    apply_push_handoff(&worker).await?;

    let rejected_ids = observed_push_ids
        .lock()
        .map_err(|_| "scheduled push id observation lock poisoned")?
        .clone();
    if rejected_ids.len() != 2 || rejected_ids[0] != rejected_ids[1] {
        return Err("pre-expiry rejection did not reuse the original schedule PushId".into());
    }
    let old_push = worker
        .interactions
        .get_push_record(&rejected_ids[0])
        .await?
        .ok_or("pending schedule push missing before retention")?;
    if old_push.delivery_state != PushDeliveryState::Pending
        || !matches!(
            old_push
                .last_outcome
                .as_ref()
                .map(|receipt| &receipt.outcome),
            Some(DeliveryOutcome::Rejected(_))
        )
    {
        return Err("test did not reach a pending rejected schedule push".into());
    }
    let service_id = String::from(old_push.target.endpoint.service_id.clone());
    let identity = crate::ApplicationFixture::new(&service_id, &service_id)
        .map_err(std::io::Error::other)?
        .with_automation_store(Arc::clone(&store))
        .with_interaction_store(Arc::clone(&worker.interactions));
    let old_link = crate::push_record_resolver::link_for(&old_push, &identity.router_identity());
    let shown_before_pruning = (identity.clone())
        .build()
        .messages()
        .push_show(collaboration_protocol::PushRecordShowParams {
            caller: old_push.target.clone(),
            reference: old_link.clone(),
        })
        .await?;
    if shown_before_pruning.record.push_id != old_push.push_id
        || shown_before_pruning.record.body != old_push.body
    {
        return Err(
            "retention fixture did not read its native push through the application".into(),
        );
    }
    let pruned_count = worker
        .interactions
        .prune_push_records(old_push.created_at + chrono::Duration::days(31), 500)
        .await?;
    if pruned_count != 1
        || worker
            .interactions
            .get_push_record(&old_push.push_id)
            .await?
            .is_some()
    {
        return Err("31-day retention did not prune the pending schedule push".into());
    }

    let old_link_result = match (identity.clone())
        .build()
        .messages()
        .push_show(collaboration_protocol::PushRecordShowParams {
            caller: old_push.target.clone(),
            reference: old_link.clone(),
        })
        .await
    {
        Ok(result) => json!({ "result": result }),
        Err(failure) => json!({
            "error": crate::collaboration_application::CollaborationRejection::published_rejection(&failure)
        }),
    };
    if old_link_result
        .pointer("/error/data/kind")
        .and_then(serde_json::Value::as_str)
        != Some("notFound")
        || !old_link_result
            .pointer("/error/data/message")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|message| message.contains("expired after 30 days"))
    {
        return Err("old schedule link did not report its expired record".into());
    }
    drop(identity);

    if let Err(error) = Box::pin(worker.step(run_id.clone())).await {
        let failed_run = store
            .lock()
            .await
            .read_run::<
                SessionRef,
                EndpointRef,
                CodexGeneration,
                collaboration_runtime::stored_run_receipt::StoredRunReceipt,
            >(&run_id)
            .await?;
        let observed_ids = observed_push_ids
            .lock()
            .map_err(|_| "scheduled push id observation lock poisoned")?
            .clone();
        let retained_origin_record = worker
            .interactions
            .get_push_record_by_router_ref(
                old_push
                    .origin_router_ref
                    .as_deref()
                    .ok_or("old ScheduleRun push had no origin reference")?,
            )
            .await?;
        return Err(format!(
            "post-retention run retry failed: {error:?}; phase={:?}; observed_push_ids={observed_ids:?}; retained_origin_record_state={:?}",
            failed_run.phase,
            retained_origin_record.map(|record| record.delivery_state),
        )
        .into());
    }
    apply_push_handoff(&worker).await?;
    let run = store
        .lock()
        .await
        .read_run::<SessionRef, EndpointRef, CodexGeneration, collaboration_runtime::stored_run_receipt::StoredRunReceipt>(&run_id)
        .await?;
    if run.phase != RunPhase::Executing {
        return Err("run retry did not proceed after its old push expired".into());
    }
    let all_push_ids = observed_push_ids
        .lock()
        .map_err(|_| "scheduled push id observation lock poisoned")?
        .clone();
    if all_push_ids.len() != 3
        || all_push_ids[0] != all_push_ids[1]
        || all_push_ids[2] == old_push.push_id
    {
        return Err("post-expiry retry did not create one fresh ScheduleRun PushId".into());
    }
    let fresh_push = worker
        .interactions
        .get_push_record(&all_push_ids[2])
        .await?
        .ok_or("fresh schedule push missing after retry")?;
    let header_facts_match = match (&old_push.header_facts, &fresh_push.header_facts) {
        (
            PushHeaderFacts::ScheduleRun {
                schedule_id: old_schedule_id,
                run_id: old_run_id,
            },
            PushHeaderFacts::ScheduleRun {
                schedule_id: fresh_schedule_id,
                run_id: fresh_run_id,
            },
        ) => old_schedule_id == fresh_schedule_id && old_run_id == fresh_run_id,
        _ => false,
    };
    if fresh_push.kind != PushKind::ScheduleRun
        || fresh_push.target != old_push.target
        || fresh_push.origin_router_ref != old_push.origin_router_ref
        || fresh_push.body != old_push.body
        || !header_facts_match
        || fresh_push.delivery_state != PushDeliveryState::Delivered
        || !matches!(
            fresh_push
                .last_outcome
                .as_ref()
                .map(|receipt| &receipt.outcome),
            Some(DeliveryOutcome::Started)
        )
    {
        return Err("fresh schedule push was not delivered after retention".into());
    }

    let interactions = Arc::clone(&worker.interactions);

    drop(worker);

    Arc::try_unwrap(interactions)
        .map_err(|_| "schedule fixture interaction owner remained shared")?
        .close()
        .await?;
    let store = Arc::try_unwrap(store).map_err(|_| "store still referenced")?;
    store.into_inner().close().await?;
    for entry in std::fs::read_dir(&root)? {
        std::fs::remove_file(entry?.path())?;
    }
    std::fs::remove_dir(root)?;
    Ok(())
}
