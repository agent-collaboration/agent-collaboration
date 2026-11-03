//! Reader-only registries borrow the authenticated session and refetch disposable generations.
use super::*;
use crate::collaboration_runtime::board_composition::central_read_copy::CentralReadCopies;

async fn replicated_head(
    runtime: &Runtime,
) -> Result<(ProjectState, FeedCursor), Box<dyn std::error::Error + Send + Sync>> {
    let writer = runtime.projects.ensure(&runtime.project).await?;
    let mut read = writer.store.snapshot().await?;
    let head = read.high_water();
    let Some(Record::Project(state)) = read.record(&runtime.project.clone().into()).await? else {
        return Err("source project".into());
    };
    read.finish().await?;
    timeout(BOUND, async {
        loop {
            if runtime
                .projects
                .replicated_through(&runtime.project, head.epoch)
                .await?
                == head
            {
                return Ok::<_, BoardError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    Ok((state, head))
}

fn reader_registry(
    runtime: &Runtime,
    root: &Path,
    source: Arc<CentralReadCopies>,
) -> Result<Arc<ProjectStoreRegistry>, Box<dyn std::error::Error + Send + Sync>> {
    let registry = Arc::new(ProjectStoreRegistry::new(
        root,
        Some(runtime.central.peer),
        Arc::new(SystemClock),
    ));
    registry.attach_central_reads(source)?;
    Ok(registry)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn machine_session_startup_removes_stale_board_read_cache_roots() -> TestResult {
    let directory = tempfile::tempdir()?;
    let router_root = directory.path().join("router");
    let stale_root = router_root
        .join("board-read-cache")
        .join("550e8400-e29b-41d4-a716-446655440001");
    tokio::fs::create_dir_all(&stale_root).await?;
    tokio::fs::write(stale_root.join("stale.marker"), b"old session").await?;
    let mut central =
        TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;

    let board = open_board_admission(&router_root, identity()?, Some(central.inputs()?)).await?;
    let stale_root_survived_startup = stale_root.exists();
    let admission = board.admission.clone();
    drop(board);
    Arc::try_unwrap(admission)
        .map_err(|_| "prepared admission still owned")?
        .close()
        .await?;
    central.stop().await?;

    assert!(
        !stale_root_survived_startup,
        "machine-session startup retained a stale board-read-cache root"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn registry_close_removes_a_released_central_read_generation() -> TestResult {
    use futures_util::FutureExt as _;

    let runtime = Box::pin(Runtime::new()).await?;
    let (state, _) = replicated_head(&runtime).await?;
    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 9)?];
    let root = runtime.directory.path().join("reader-close");
    tokio::fs::create_dir_all(&root).await?;
    let source = CentralReadCopies::new(
        &root,
        runtime.requests.clone(),
        runtime.central.receiver,
        runtime.readiness.clone(),
    );
    let registry = reader_registry(&runtime, &root, Arc::new(source))?;
    let handle = registry
        .read_project(&runtime.project)
        .await?
        .ok_or("central reader")?;
    let snapshot = handle.snapshot().await?;
    let copy = copy_files(&root.join("board-read-cache"))?
        .into_iter()
        .next()
        .ok_or("installed central copy")?;
    let generation_root = copy
        .parent()
        .and_then(Path::parent)
        .ok_or("central generation root")?
        .to_owned();
    assert!(generation_root.is_dir());

    let registry = Arc::try_unwrap(registry).map_err(|_| "reader registry still owned")?;
    let mut close = Box::pin(registry.close());
    assert!(
        close.as_mut().now_or_never().is_none(),
        "registry close completed while a reader still owned the generation"
    );
    assert!(
        generation_root.is_dir(),
        "registry close removed a generation while its reader was open"
    );

    snapshot.finish().await?;
    drop(handle);
    close.await?;

    assert!(
        !generation_root.exists(),
        "registry close retained a released central generation directory"
    );
    runtime.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn startup_sweep_removes_stale_root_and_keeps_an_open_reader_generation() -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    let (state, head) = replicated_head(&runtime).await?;
    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 9)?];
    let root = runtime.directory.path().join("reader-sweep");
    let stale_root = root
        .join("board-read-cache")
        .join("550e8400-e29b-41d4-a716-446655440001");
    tokio::fs::create_dir_all(&stale_root).await?;
    tokio::fs::write(stale_root.join("stale.marker"), b"old session").await?;
    let source = Arc::new(CentralReadCopies::new(
        &root,
        runtime.requests.clone(),
        runtime.central.receiver,
        runtime.readiness.clone(),
    ));
    let registry = reader_registry(&runtime, &root, source.clone())?;
    let handle = registry
        .read_project(&runtime.project)
        .await?
        .ok_or("central reader")?;
    let snapshot = handle.snapshot().await?;
    let copy = copy_files(&root.join("board-read-cache"))?
        .into_iter()
        .next()
        .ok_or("installed central copy")?;
    let generation_root = copy
        .parent()
        .and_then(Path::parent)
        .ok_or("central generation root")?
        .to_owned();
    let session_root = generation_root
        .parent()
        .and_then(Path::parent)
        .ok_or("current session root")?
        .to_owned();

    source.sweep_stale_session_roots().await;

    assert!(!stale_root.exists(), "stale session root survived sweep");
    assert!(
        session_root.is_dir() && generation_root.is_dir(),
        "startup sweep removed the live session root with an open reader"
    );
    assert_eq!(snapshot.high_water(), head);
    snapshot.finish().await?;
    drop(handle);
    Arc::try_unwrap(registry)
        .map_err(|_| "reader registry still owned")?
        .close()
        .await?;
    assert!(
        !generation_root.exists(),
        "closed generation directory remained"
    );
    runtime.close().await?;
    Ok(())
}

async fn assert_partial_pull(move_between_pages: bool) -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    let (state, original_head) = replicated_head(&runtime).await?;
    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 9)?];
    let root = runtime.directory.path().join("partial-reader");
    tokio::fs::create_dir_all(&root).await?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let source = CentralReadCopies::new(
        &root,
        runtime.requests.clone(),
        runtime.central.receiver,
        runtime.readiness.clone(),
    );
    let (source, pause) = source.pause_after_staged_page_for_test();
    let registry = reader_registry(&runtime, &root, Arc::new(source))?;
    let reader = registry.clone();
    let project = runtime.project.clone();
    let pulling = tokio::spawn(async move { reader.read_project(&project).await });
    let observed = timeout(BOUND, pause.staged).await??;
    assert!(
        observed.receipt.rows > 0,
        "actual first page staged no rows"
    );
    assert!(
        observed.receipt.next.is_some(),
        "fixture did not reach the between-pages boundary"
    );
    assert!(
        !pulling.is_finished(),
        "partial generation became readable before verification"
    );
    // Inspect only the application catalog of the actual unfinished target.
    // Page rows live in SDK staging until verified publication.
    {
        use sqlx::Connection as _;
        let mut target = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&observed.target)
                .read_only(true),
        )
        .await?;
        let application_tables: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name='project_state'")
                .fetch_one(&mut target)
                .await?;
        assert_eq!(
            application_tables, 0,
            "unfinished target exposed project tables"
        );
        target.close().await?;
    }
    let expected_head = if move_between_pages {
        let context = runtime.admission.runtime_test_context();
        let writer = runtime.projects.ensure(&runtime.project).await?;
        let mut changed = state.clone();
        changed.revision = changed.revision.successor()?;
        changed.envelope.request = None;
        let mut write = writer.store.begin_admission(context.origin).await?;
        let head = write.append_record(Record::Project(changed)).await?;
        write.commit().await?;
        drop(writer);
        let (_, replicated) = replicated_head(&runtime).await?;
        assert_eq!(replicated, head);
        pause.proceed.send(()).map_err(|_| "page pause closed")?;
        let mut moved = pause.moved;
        timeout(BOUND, async {
            while moved.borrow().is_none() {
                moved.changed().await?;
            }
            Ok::<_, tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        assert!(moved.borrow().is_some(), "no actual SDK Moved response");
        let result = timeout(BOUND, pulling).await??;
        match result {
            Ok(Some(handle)) => {
                let snapshot = handle.snapshot().await?;
                assert_eq!(
                    snapshot.high_water(),
                    head,
                    "mixed old/new pages were published"
                );
                snapshot.finish().await?;
            }
            Err(collaboration_runtime::ProjectRegistryError::Unavailable { .. }) => {}
            _ => return Err("unexpected moved-pull outcome".into()),
        }
        head
    } else {
        // Abandon the actual in-progress installer, drain its owning future and
        // drop the entire reader generation owner before rebuilding over disk.
        pulling.abort();
        assert!(pulling.await.is_err_and(|error| error.is_cancelled()));
        drop(pause.proceed);
        drop(pause.moved);
        original_head
    };
    Arc::try_unwrap(registry)
        .map_err(|_| "first reader owner retained")?
        .close()
        .await?;
    // Cleanup may remove an abandoned or released file. Its actual partial
    // existence was established while the staging receipt was held above.
    // A new process-scoped source cannot reuse the old partial generation.
    let source = CentralReadCopies::new(
        &root,
        runtime.requests.clone(),
        runtime.central.receiver,
        runtime.readiness.clone(),
    );
    let registry = reader_registry(&runtime, &root, Arc::new(source))?;
    let handle = timeout(BOUND, registry.read_project(&runtime.project))
        .await??
        .ok_or("fresh verified copy")?;
    let mut snapshot = handle.snapshot().await?;
    assert_eq!(snapshot.high_water(), expected_head);
    let Some(Record::Project(read_state)) =
        snapshot.record(&runtime.project.clone().into()).await?
    else {
        return Err("reopened copy project missing".into());
    };
    assert_eq!(
        read_state.revision.get(),
        state.revision.get() + u64::from(move_between_pages)
    );
    snapshot.finish().await?;
    drop(handle);
    assert!(
        !root.join("projects").exists(),
        "partial recovery constructed a writer"
    );
    Arc::try_unwrap(registry)
        .map_err(|_| "fresh reader owner retained")?
        .close()
        .await?;
    runtime.close().await
}

#[tokio::test]
async fn a_position_move_between_actual_snapshot_pages_never_publishes_a_mixed_generation()
-> TestResult {
    assert_partial_pull(true).await
}
#[tokio::test]
async fn an_interrupted_native_page_install_is_refetched_after_owning_reopen() -> TestResult {
    assert_partial_pull(false).await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_fresh_nonholder_without_any_local_project_reads_unknown_authority_from_central()
-> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    let writer = runtime.projects.ensure(&runtime.project).await?;
    let mut read = writer.store.snapshot().await?;
    let head = read.high_water();
    let Some(Record::Project(state)) = read.record(&runtime.project.clone().into()).await? else {
        return Err("source project".into());
    };
    read.finish().await?;
    timeout(BOUND, async {
        loop {
            if runtime
                .projects
                .replicated_through(&runtime.project, head.epoch)
                .await?
                == head
            {
                return Ok::<_, BoardError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    drop(writer);
    let mut entry = directory_entry(&state, 9)?;
    entry.lease_side = DirectorySide::NotYetReported;
    *runtime.central.read_copy_entries.lock().await = vec![entry];
    let upgrades = runtime.central.auth.upgrades.load(Ordering::SeqCst);
    let root = runtime.directory.path().join("reader-only");
    tokio::fs::create_dir_all(&root).await?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let registry = Arc::new(ProjectStoreRegistry::new(
        &root,
        Some(runtime.central.peer),
        Arc::new(SystemClock),
    ));
    registry.attach_central_reads(Arc::new(CentralReadCopies::new(
        &root,
        runtime.requests.clone(),
        runtime.central.receiver,
        runtime.readiness.clone(),
    )))?;
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: runtime.fleet.clone(),
        identity: identity()?,
    })?);
    let application = (ApplicationFixture::new(
        "550e8400-e29b-41d4-a716-446655440000",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone()))
    .build();
    assert!(!root.join("projects").exists());
    let view = timeout(
        BOUND,
        application.board().view_work(ProjectViewRequest {
            project_id: runtime.project.clone(),
            include_removed: false,
        }),
    )
    .await??;
    assert_eq!(view.project, runtime.project);
    assert_eq!(view.as_of, head);
    assert_eq!(view.replicated_through, head);
    assert_eq!(view.source, CopySource::ReadCopy {});
    assert_eq!(view.current_writer, CurrentWriter::Unknown {});
    assert_eq!(view.writer_observation, None);
    assert!(
        !root.join("projects").exists(),
        "central read created a local project runtime"
    );
    assert!(registry.project_ids().await?.is_empty());
    assert!(!copy_files(&root.join("board-read-cache"))?.is_empty());
    assert_eq!(
        runtime.central.auth.upgrades.load(Ordering::SeqCst),
        upgrades,
        "central read opened another authenticated connection"
    );
    drop(application);
    drop(admission);
    Arc::try_unwrap(registry)
        .map_err(|_| "reader registry still owned")?
        .close()
        .await?;
    runtime.close().await
}
