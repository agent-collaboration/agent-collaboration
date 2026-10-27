//! U66: schedules retain the long-lived occurrence property retired from wakes.
use super::fixture::{FixtureTiming, provider_worker_fixture_at};
use super::*;
use crate::collaboration_application::CollaborationRejection;
use collaboration_protocol::{MachineId, PushRecordDraft, PushRecordShowParams, RouterOriginRef};

macro_rules! ensure {
    ($condition:expr $(,)?) => { ensure!($condition, "assertion failed: {}", stringify!($condition)); };
    ($condition:expr, $($context:tt)+) => {
        if !$condition { return Err(format!($($context)+).into()); }
    };
}
macro_rules! ensure_eq {
    ($left:expr, $right:expr $(,)?) => { ensure_eq!($left, $right, "values differ"); };
    ($left:expr, $right:expr, $($context:tt)+) => {{
        let left = &$left; let right = &$right;
        if left != right { return Err(format!("{}: left={left:?}, right={right:?}", format_args!($($context)+)).into()); }
    }};
}
macro_rules! ensure_ne {
    ($left:expr, $right:expr $(,)?) => {
        ensure!($left != $right, "values must differ");
    };
}
#[tokio::test]
async fn same_schedule_delivers_a_fresh_occurrence_after_its_old_push_is_age_pruned()
-> TestResult<()> {
    let now = chrono::Utc::now();
    let interval = chrono::Duration::days(31);
    let timing = FixtureTiming {
        anchor_ms: (now - chrono::Duration::days(62)).timestamp_millis(),
        interval_seconds: 31 * 24 * 60 * 60,
    };
    let (root, store, worker, first_run, observed) = provider_worker_fixture_at(
        FakeSubmissionPlan::Accept,
        FakeSettlementPlan::Completed,
        timing,
    )
    .await?;
    let interactions = Arc::clone(&worker.interactions);
    let first = store.lock().await.read_run::<SessionRef, EndpointRef, CodexGeneration, collaboration_runtime::stored_run_receipt::StoredRunReceipt>(&first_run).await?;
    let inputs = first.inputs.as_ref().ok_or("captured schedule inputs")?;
    let ExecutionDestination::OwnedThread { target, .. } =
        &inputs.execution_configuration.destination
    else {
        return Err("prepared owned-thread schedule fixture required".into());
    };
    let target = target.clone();
    let schedule_id = first.schedule_id.clone();
    let definition_before = store
        .lock()
        .await
        .inspect_schedule::<SessionRef, EndpointRef>(&schedule_id)
        .await?
        .record;
    let first_push_id: PushId = uuid::Uuid::now_v7().to_string().try_into()?;
    // Literal historical clock at the existing push-preparation boundary. The
    // actual worker must reuse this prepared push and send it through its route.
    let origin = RouterOriginRef::ScheduleRun {
        schedule_id: schedule_id.clone(),
        run_id: first_run.clone(),
    };
    interactions
        .insert_push_record(PushRecordDraft {
            push_id: first_push_id.clone(),
            kind: PushKind::ScheduleRun,
            origin: PushOrigin::Router(PushKind::ScheduleRun),
            origin_router_ref: Some(origin.canonical_string()?),
            target: target.clone(),
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::ScheduleRun {
                schedule_id: schedule_id.clone(),
                run_id: first_run.clone(),
            },
            body: Some(inputs.instruction_text.as_str().to_owned()),
            activity: None,
            mode: None,
            guard: None,
            created_at: chrono::DateTime::from_timestamp_millis(first.due_at_ms)
                .ok_or("first due")?,
        })
        .await?;
    for _ in 0..4 {
        worker.step(first_run.clone()).await?;
        apply_push_handoff(&worker).await?;
    }
    let first = store.lock().await.read_run::<SessionRef, EndpointRef, CodexGeneration, collaboration_runtime::stored_run_receipt::StoredRunReceipt>(&first_run).await?;
    ensure_eq!(first.phase, agent_automation::RunPhase::Finished);
    let old = interactions
        .get_push_record(&first_push_id)
        .await?
        .ok_or("first push")?;
    ensure_eq!(old.delivery_state, PushDeliveryState::Delivered);
    ensure!(old.created_at < now - chrono::Duration::days(30));
    ensure_eq!(
        interactions.prune_push_records(now, 500).await?,
        1,
        "historical native push prune count"
    );
    ensure!(
        interactions
            .get_push_record(&first_push_id)
            .await?
            .is_none()
    );
    let service_id = target.endpoint.service_id.clone();
    let service = String::from(service_id.clone());
    let identity = crate::ServiceIdentity::new(&service, &service)?
        .with_automation_store(Arc::clone(&store))
        .with_interaction_store(Arc::clone(&interactions));
    let application = crate::CollaborationApplication::new(identity);
    let old_link = RouterLink::new(MachineId::from(service_id.clone()), first_push_id.clone());
    let error = application
        .messages()
        .push_show(PushRecordShowParams {
            caller: target.clone(),
            reference: old_link.to_string(),
        })
        .await
        .err()
        .ok_or("pruned link was still readable")?;
    ensure_eq!(
        error
            .published_rejection()
            .data
            .and_then(|data| data.get("kind").cloned()),
        Some(json!("notFound"))
    );

    // A new timer occurrence of the same unchanged schedule, not a retry of the
    // first Run, is admitted after the previous push has really been deleted.
    let second_run = store
        .lock()
        .await
        .enqueue_due_run::<SessionRef, EndpointRef>(&schedule_id, now.timestamp_millis())
        .await?
        .ok_or("second occurrence")?;
    ensure_ne!(second_run, first_run);
    ensure!(
        matches!(store.lock().await.admit_waiting_run::<SessionRef, EndpointRef>(&schedule_id, now.timestamp_millis()).await?, automation_storage::RunAdmission::Admitted { run_id, .. } if run_id == second_run)
    );
    for _ in 0..4 {
        worker.step(second_run.clone()).await?;
        apply_push_handoff(&worker).await?;
    }
    let second = store.lock().await.read_run::<SessionRef, EndpointRef, CodexGeneration, collaboration_runtime::stored_run_receipt::StoredRunReceipt>(&second_run).await?;
    ensure_eq!(second.phase, agent_automation::RunPhase::Finished);
    ensure!(second.due_at_ms - first.due_at_ms >= interval.num_milliseconds());
    let new_origin = RouterOriginRef::ScheduleRun {
        schedule_id: schedule_id.clone(),
        run_id: second_run.clone(),
    };
    let fresh = interactions
        .get_push_record_by_origin_reference(&new_origin)
        .await?
        .ok_or("second actual occurrence push")?;
    ensure_ne!(fresh.push_id, first_push_id);
    ensure_eq!(fresh.delivery_state, PushDeliveryState::Delivered);
    ensure_eq!(fresh.kind, PushKind::ScheduleRun);
    ensure_eq!(fresh.target, target);
    ensure_eq!(
        fresh.header_facts,
        PushHeaderFacts::ScheduleRun {
            schedule_id: schedule_id.clone(),
            run_id: second_run
        }
    );
    let ids = observed.lock().map_err(|_| "route observations")?.clone();
    ensure_eq!(
        ids.iter().filter(|id| **id == fresh.push_id).count(),
        1,
        "second occurrence native correlation count"
    );
    ensure!(ids.iter().any(|id| id == &first_push_id));
    let current = store
        .lock()
        .await
        .inspect_schedule::<SessionRef, EndpointRef>(&schedule_id)
        .await?
        .record;
    ensure_eq!(
        serde_json::to_value(&current.definition)?,
        serde_json::to_value(&definition_before.definition)?
    );
    ensure_eq!(current.change_id, definition_before.change_id);
    ensure_eq!(current.anchor_at_ms, definition_before.anchor_at_ms);
    ensure_eq!(current.created_at_ms, definition_before.created_at_ms);
    ensure!(current.next_due_at_ms > definition_before.next_due_at_ms);
    let shown = application
        .messages()
        .push_show(PushRecordShowParams {
            caller: target,
            reference: RouterLink::new(MachineId::from(service_id), fresh.push_id.clone())
                .to_string(),
        })
        .await?;
    ensure_eq!(shown.record.push_id, fresh.push_id);
    ensure_eq!(shown.record.body, fresh.body);
    drop(application);
    drop(worker);
    Arc::try_unwrap(interactions)
        .map_err(|_| "shared native interaction fixture")?
        .close()
        .await?;
    Arc::try_unwrap(store)
        .map_err(|_| "shared schedule fixture")?
        .into_inner()
        .close()
        .await?;
    for entry in std::fs::read_dir(&root)? {
        std::fs::remove_file(entry?.path())?;
    }
    std::fs::remove_dir(root)?;
    Ok(())
}
