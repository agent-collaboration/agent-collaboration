use super::*;

struct LocalTaskSlot {
    task: Option<JoinHandle<ProofResult<()>>>,
    release: Option<oneshot::Sender<()>>,
}
async fn reject_unjoined_task(slot: &mut LocalTaskSlot) -> ProofResult<()> {
    if let Some(task) = slot.task.take() {
        if let Some(release) = slot.release.take() {
            let _released = release.send(());
        }
        let result = task.await;
        return Err(
            format!("finalizer skipped task join; fallback join result: {result:?}").into(),
        );
    }
    Ok(())
}

#[tokio::test]
async fn lone_cleanup_error_is_preserved_and_verify_observes_joined_local_task() -> ProofResult<()>
{
    let directory = tempfile::tempdir()?;
    let marker = directory.path().join("joined-task-marker");
    let task_marker = marker.clone();
    let (release, released) = oneshot::channel();
    let task = tokio::spawn(async move {
        released.await?;
        std::fs::write(task_marker, b"local task completed before verify")?;
        Ok::<(), MatrixError>(())
    });
    let mut slot = LocalTaskSlot {
        task: Some(task),
        release: Some(release),
    };
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let identity = Arc::new(());
    let original_identity = Arc::clone(&identity);
    let result = run_matrix_scenario(
        &mut slot,
        async move |_: &mut &mut LocalTaskSlot| -> ProofResult<()> {
            scenario_calls.record("scenario")
        },
        move |slot| async move {
            cleanup_calls.record("cleanup")?;
            let release = slot.release.take().ok_or("local task release missing")?;
            let release_result = release
                .send(())
                .map_err(|_| "local task release receiver closed");
            let joined = slot.task.take().ok_or("local task handle missing")?.await;
            release_result?;
            joined??;
            Err(injected(FailureCategory::ResourceCleanup, identity))
        },
        || {
            verify_calls.record("verify")?;
            if std::fs::read(&marker)? != b"local task completed before verify" {
                return Err("verify ran without completed local-task evidence".into());
            }
            Ok(())
        },
    )
    .await;
    reject_unjoined_task(&mut slot).await?;
    calls.require_once_in_order()?;
    let error = result.err().ok_or("cleanup error became success")?;
    require_injected(&error, FailureCategory::ResourceCleanup, &original_identity)
}

#[tokio::test]
async fn lone_config_error_is_returned_unchanged() -> ProofResult<()> {
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let identity = Arc::new(());
    let original_identity = Arc::clone(&identity);
    let result = run_matrix_scenario(
        (),
        async move |_: &mut ()| -> ProofResult<()> { scenario_calls.record("scenario") },
        move |_| async move { cleanup_calls.record("cleanup") },
        move || {
            verify_calls.record("verify")?;
            Err(injected(FailureCategory::ConfigVerification, identity))
        },
    )
    .await;
    calls.require_once_in_order()?;
    require_injected(
        &result.err().ok_or("config error became success")?,
        FailureCategory::ConfigVerification,
        &original_identity,
    )
}

#[tokio::test]
async fn both_cleanup_categories_remain_named_and_inspectable() -> ProofResult<()> {
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let resource_identity = Arc::new(());
    let config_identity = Arc::new(());
    let original_resource = Arc::clone(&resource_identity);
    let original_config = Arc::clone(&config_identity);
    let result = run_matrix_scenario(
        (),
        async move |_: &mut ()| -> ProofResult<()> { scenario_calls.record("scenario") },
        move |_| async move {
            cleanup_calls.record("cleanup")?;
            Err(injected(
                FailureCategory::ResourceCleanup,
                resource_identity,
            ))
        },
        move || {
            verify_calls.record("verify")?;
            Err(injected(
                FailureCategory::ConfigVerification,
                config_identity,
            ))
        },
    )
    .await;
    calls.require_once_in_order()?;
    let error = result.err().ok_or("both cleanup errors became success")?;
    let failures = error
        .downcast_ref::<CleanupFailures>()
        .ok_or("cleanup categories were not retained together")?;
    require_injected(
        &failures.resource_cleanup,
        FailureCategory::ResourceCleanup,
        &original_resource,
    )?;
    require_injected(
        &failures.config_verification,
        FailureCategory::ConfigVerification,
        &original_config,
    )
}

#[tokio::test]
async fn scenario_and_cleanup_error_remain_named_and_inspectable() -> ProofResult<()> {
    let scenario_identity = Arc::new(());
    let resource_identity = Arc::new(());
    let original_scenario = Arc::clone(&scenario_identity);
    let original_resource = Arc::clone(&resource_identity);
    let result = run_matrix_scenario(
        (),
        async move |_: &mut ()| -> ProofResult<()> {
            Err(injected(FailureCategory::Scenario, scenario_identity))
        },
        move |_| async move {
            Err(injected(
                FailureCategory::ResourceCleanup,
                resource_identity,
            ))
        },
        || Ok(()),
    )
    .await;
    let error = result
        .err()
        .ok_or("scenario plus cleanup error became success")?;
    let failure = error
        .downcast_ref::<ScenarioCleanupFailure>()
        .ok_or("scenario and cleanup categories were flattened")?;
    require_injected(
        &failure.scenario_error,
        FailureCategory::Scenario,
        &original_scenario,
    )?;
    require_injected(
        &failure.cleanup_error,
        FailureCategory::ResourceCleanup,
        &original_resource,
    )
}

#[tokio::test]
async fn scenario_and_both_cleanup_errors_preserve_all_three_originals() -> ProofResult<()> {
    let scenario_identity = Arc::new(());
    let resource_identity = Arc::new(());
    let config_identity = Arc::new(());
    let original_scenario = Arc::clone(&scenario_identity);
    let original_resource = Arc::clone(&resource_identity);
    let original_config = Arc::clone(&config_identity);
    let result = run_matrix_scenario(
        (),
        async move |_: &mut ()| -> ProofResult<()> {
            Err(injected(FailureCategory::Scenario, scenario_identity))
        },
        move |_| async move {
            Err(injected(
                FailureCategory::ResourceCleanup,
                resource_identity,
            ))
        },
        move || {
            Err(injected(
                FailureCategory::ConfigVerification,
                config_identity,
            ))
        },
    )
    .await;
    let error = result.err().ok_or("three failures became success")?;
    let failure = error
        .downcast_ref::<ScenarioCleanupFailure>()
        .ok_or("scenario failure wrapper missing")?;
    let cleanup = failure
        .cleanup_error
        .downcast_ref::<CleanupFailures>()
        .ok_or("both cleanup categories missing")?;
    require_injected(
        &failure.scenario_error,
        FailureCategory::Scenario,
        &original_scenario,
    )?;
    require_injected(
        &cleanup.resource_cleanup,
        FailureCategory::ResourceCleanup,
        &original_resource,
    )?;
    require_injected(
        &cleanup.config_verification,
        FailureCategory::ConfigVerification,
        &original_config,
    )
}

#[tokio::test]
async fn panic_keeps_original_payload_even_when_both_cleanup_callbacks_fail() -> ProofResult<()> {
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let identity = Arc::new(());
    let original_identity = Arc::clone(&identity);
    let result = AssertUnwindSafe(run_matrix_scenario(
        (),
        async move |_: &mut ()| -> ProofResult<()> {
            scenario_calls.record("scenario")?;
            resume_unwind(Box::new(PanicMarker { identity }))
        },
        move |_| async move {
            cleanup_calls.record("cleanup")?;
            Err(injected(FailureCategory::ResourceCleanup, Arc::new(())))
        },
        move || {
            verify_calls.record("verify")?;
            Err(injected(FailureCategory::ConfigVerification, Arc::new(())))
        },
    ))
    .catch_unwind()
    .await;
    calls.require_once_in_order()?;
    require_panic(
        result
            .err()
            .ok_or("cleanup failure replaced scenario panic")?,
        &original_identity,
    )
}

#[tokio::test]
async fn scenario_invocation_panic_is_caught_before_awaited_cleanup() -> ProofResult<()> {
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let identity = Arc::new(());
    let original_identity = Arc::clone(&identity);
    let scenario = move |_: &mut ()| -> Ready<ProofResult<()>> {
        let _recorded = scenario_calls.record("scenario");
        resume_unwind(Box::new(PanicMarker { identity }))
    };
    let result = AssertUnwindSafe(run_matrix_scenario(
        (),
        scenario,
        move |_| async move { cleanup_calls.record("cleanup") },
        move || verify_calls.record("verify"),
    ))
    .catch_unwind()
    .await;
    calls.require_once_in_order()?;
    require_panic(
        result
            .err()
            .ok_or("invocation panic became normal result")?,
        &original_identity,
    )
}
