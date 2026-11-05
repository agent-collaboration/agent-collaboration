//! Two real Central lifetimes exercise unchanged rich epoch-two application history.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    host_entrypoints::HOST_BASE_PREFIX,
    host_process::OwnedHost,
    native_snapshot_fixture::{NativeFixture, native_fixture},
    offline_cli_fixture::{RunFixture, TestResult},
    recovery_rows::require,
    rich_source_messages::{self, RichMessages},
    rich_source_records::{self, RichRecords},
};
use board_central_test_support::TestCentral;
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema};
use board_staging_cutover::{NativeProjectSnapshot, PreparedProjectBundle, SelectedWriterFile};
use collaboration_client::CollaborationClient;
use fleet_sync_lease::LeaseHolder;
use fleet_sync_protocol::Epoch;
use fleet_sync_writer::{ReplicatedDatabase, WriterOptions};
use message_board::{ActorOrigin, EpochLine};
use runtime_primitives::SystemClock;
use std::{os::unix::fs::PermissionsExt, sync::Arc};

#[tokio::test]
async fn rich_real_l2_history_survives_cutover_without_resetting_line_two_or_replaying_writes()
-> TestResult {
    // Arrange real old/new Central lifetimes and one explicitly selected former writer.
    let base = tempfile::Builder::new()
        .prefix(HOST_BASE_PREFIX)
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700))?;
    let parent = std::fs::canonicalize(base.path())?;
    let host = CutoverHostFixture::new_in(&parent).map_err(boxed)?;
    let genesis = native_fixture(&host).await.map_err(boxed)?;
    let origin = ActorOrigin {
        installation: String::from(host.service.clone()).try_into()?,
        machine: "rich former writer".to_owned().try_into()?,
    };
    let old_area = tempfile::tempdir()?;
    let new_area = tempfile::tempdir()?;
    let old_central = TestCentral::start_with_peer(old_area.path(), &origin, host.peer).await?;
    let new_central = TestCentral::start_with_peer(new_area.path(), &origin, host.peer).await?;
    let old_artifacts = tempfile::tempdir()?;
    let new_artifacts = tempfile::tempdir()?;
    let observed = async {
        let old_run = RunFixture::new(
            &host,
            &genesis,
            old_central.base_url(),
            old_artifacts.path(),
        )?;
        require(
            old_run.execute().await?.status.success(),
            "old source bootstrap import failed",
        )?;
        let old_bundle = PreparedProjectBundle::open(&old_run.bundle)?;
        super::direct_central_enrollment::install(&old_central, &host)?;
        let (records, messages) =
            populate_source(&host, &old_central, &old_run, &old_bundle).await?;
        let (source, line_two) = capture_actual_source(&host, &genesis).await?;
        require(
            source.cursor.epoch.get() == 2,
            "source application history did not reach L2",
        )?;
        require(
            line_two.inherited_through == genesis.cursor.position,
            "source line two does not retain the e1 prefix",
        )?;
        require_nonempty_tables(&source)?;
        let run = RunFixture::from_preserved_fleet(
            &host,
            &source,
            new_central.base_url(),
            new_artifacts.path(),
            old_bundle.manifest().fleet.clone(),
        )?;
        // Act: publish the exact source into the new Central, archive it, then ordinary Claim.
        require(
            run.execute().await?.status.success(),
            "rich source cutover publication failed",
        )?;
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        require(
            bundle.manifest().source.manifest == source.manifest,
            "sealed source manifest changed actual SDK facts",
        )?;
        require(
            bundle.manifest().fleet == old_bundle.manifest().fleet,
            "preserved Fleet graph was reconstructed or retimestamped",
        )?;
        super::direct_central_enrollment::install(&new_central, &host)?;
        install_target(&host, &new_central, &run, &bundle).await?;
        let mut offline = host.acquire().map_err(boxed)?;
        let installed = NativeProjectSnapshot::open(
            &mut offline,
            SelectedWriterFile {
                project: source.selected.project.clone(),
                peer: host.peer,
                file_epoch: Epoch::new(2)?,
            },
        )
        .await?;
        // Assert against the independent pre-export SDK manifest, never a rewritten artifact.
        let checked = async {
            installed.verify_replicated_tables(&source.manifest)?;
            require(
                installed.preflight().application_epoch.get() == 2,
                "target application epoch changed",
            )?;
            require(
                installed.preflight().high_water == source.cursor,
                "target application history changed before a new write",
            )?;
            require(
                installed.preflight().line_two.as_ref() == Some(&line_two),
                "cutover reset line-two inheritance",
            )?;
            require(
                installed.preflight().latest_record_at == source.record_time,
                "cutover replaced the source record time",
            )?;
            require(
                bundle
                    .manifest()
                    .source_fingerprint
                    .matches(&bundle.manifest().backup_directory)?,
                "rich source backup changed",
            )?;
            require(
                old_bundle
                    .manifest()
                    .source_fingerprint
                    .matches(&old_bundle.manifest().backup_directory)?,
                "unrelated prior backup changed",
            )?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = installed.close().await;
        checked?;
        closed?;
        drop(offline);
        super::rich_source_readback::verify(
            &host,
            &new_central,
            &run,
            &bundle,
            &records,
            &messages,
        )
        .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let new_stopped = new_central.shutdown().await;
    let old_stopped = old_central.shutdown().await;
    if let Err(error) = observed {
        let root = host._area.keep();
        let _base = base.keep();
        let prior = old_artifacts.keep();
        let prepared = new_artifacts.keep();
        return Err(format!(
            "{error}; owned rich source {}; prior {}; prepared {}",
            root.display(),
            prior.display(),
            prepared.display()
        )
        .into());
    }
    new_stopped?;
    old_stopped?;
    Ok(())
}

async fn populate_source(
    host_root: &CutoverHostFixture,
    central: &TestCentral,
    run: &RunFixture,
    bundle: &PreparedProjectBundle,
) -> TestResult<(RichRecords, RichMessages)> {
    let host = OwnedHost::start(&host_root.root, &bundle.manifest().header.declared_owner).await?;
    let observed = async {
        host.await_ready().await?;
        run.renew_owner(central.base_url())?;
        require(
            run.execute_claim().await?.status.success(),
            "old source Claim failed",
        )?;
        let directory = codex_router_host::router_collaboration_directory(&host_root.root);
        let client = CollaborationClient::connect(&directory, "rich-source-builder", "1").await?;
        let records = rich_source_records::seed(
            &client,
            bundle.manifest().source.selected.project.clone(),
            &bundle.manifest().header.declared_owner,
        )
        .await?;
        let messages = rich_source_messages::seed(&client, &records).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((records, messages))
    }
    .await;
    let stopped = host.stop().await;
    let records = observed?;
    stopped?;
    Ok(records)
}

async fn install_target(
    host_root: &CutoverHostFixture,
    central: &TestCentral,
    run: &RunFixture,
    bundle: &PreparedProjectBundle,
) -> TestResult {
    let host = OwnedHost::start(&host_root.root, &bundle.manifest().header.declared_owner).await?;
    let observed = async {
        host.await_ready().await?;
        run.renew_owner(central.base_url())?;
        require(
            run.execute_claim().await?.status.success(),
            "rich target Claim failed",
        )
    }
    .await;
    let stopped = host.stop().await;
    observed?;
    stopped?;
    Ok(())
}

async fn capture_actual_source(
    host: &CutoverHostFixture,
    genesis: &NativeFixture,
) -> TestResult<(NativeFixture, EpochLine)> {
    let path = genesis.path.with_file_name("e2.sqlite");
    let database = ReplicatedDatabase::open_with(
        &path,
        genesis.manifest.database,
        BoardProjectSchema::new()?,
        LeaseHolder::provisional(genesis.manifest.database, host.peer, BOARD_LEASE_POLICY),
        Arc::new(SystemClock),
        WriterOptions {
            allow_provisional_writes: false,
        },
    )
    .await?;
    let mut snapshot = database.snapshot().await?;
    let manifest = snapshot.manifest().await?;
    snapshot.close().await?;
    database.close().await;
    drop(database);
    let mut offline = host.acquire().map_err(boxed)?;
    let selected = SelectedWriterFile {
        project: genesis.selected.project.clone(),
        peer: host.peer,
        file_epoch: Epoch::new(2)?,
    };
    let source = NativeProjectSnapshot::open(&mut offline, selected.clone()).await?;
    let preflight = source.preflight().clone();
    let closed = source.close().await;
    closed?;
    require(
        preflight.manifest == manifest,
        "tool preflight differs from the independent SDK snapshot",
    )?;
    let line = preflight.line_two.ok_or("real L2 source line missing")?;
    Ok((
        NativeFixture {
            selected,
            manifest,
            record_time: preflight.latest_record_at,
            cursor: preflight.high_water,
            path,
            scope: preflight.project_scope,
        },
        line,
    ))
}

fn require_nonempty_tables(source: &NativeFixture) -> TestResult {
    for name in [
        "workstreams",
        "tasks",
        "task_links",
        "task_link_events",
        "subtasks",
        "threads",
        "thread_refilings",
        "posts",
        "post_references",
        "thread_participation",
        "role_assignments",
        "facts",
        "plan_revisions",
        "read_marks",
        "thread_watches",
        "project_watches",
        "subscription_memberships",
        "request_decisions",
        "project_feed",
    ] {
        require(
            source
                .manifest
                .tables
                .iter()
                .any(|table| table.table.as_str() == name && table.row_count > 0),
            &format!("real source table {name} is empty"),
        )?;
    }
    Ok(())
}

fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}
