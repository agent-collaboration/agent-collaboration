//! A bootstrapped file keeps its seed position until its first captured write.
use crate::{
    claim_session::ClaimSession, cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::native_fixture,
};
use board_central_test_support::TestCentral;
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema};
use board_staging_cutover::{NativeProjectSnapshot, SelectedWriterFile, reframe_import_manifest};
use fleet_native_writer::StagingStore;
use fleet_project_copy::SnapshotInstall;
use fleet_project_writer::ReplicatedDatabase;
use fleet_protocol::{Epoch, InstallId};
use message_board::ActorOrigin;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}

#[tokio::test]
async fn a_real_bootstrapped_e2_without_new_writes_keeps_its_e1_snapshot_and_exact_table_hashes()
-> TestResult {
    installed_before_activation(false).await
}

#[tokio::test]
async fn a_pending_intent_keeps_exact_native_table_hashes_before_application_activation()
-> TestResult {
    installed_before_activation(true).await
}

async fn installed_before_activation(with_pending_intent: bool) -> TestResult {
    // Arrange a real e1 source, then import it and obtain e2 from the actual registry.
    let fixture = CutoverHostFixture::new(None).map_err(boxed)?;
    let mut source = native_fixture(&fixture).await.map_err(boxed)?;
    if with_pending_intent {
        crate::pending_intent_fixture::add_pending_intent(&fixture, &mut source).await?;
        assert_eq!(
            source
                .manifest
                .tables
                .iter()
                .find(|table| table.table.as_str() == "notify_intents")
                .ok_or("intent table absent")?
                .row_count,
            1
        );
    }
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "bootstrap position regression".to_owned().try_into()?,
    };
    let central_root = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_root.path(), &origin, fixture.peer).await?;
    let result = async {
        let mut offline = fixture.acquire().map_err(boxed)?;
        let mut pinned = NativeProjectSnapshot::open(&mut offline, source.selected.clone()).await?;
        let manifest = pinned.preflight().manifest.clone();
        assert_eq!(manifest, source.manifest);
        let mut pages = Vec::new();
        while let Some(page) = pinned.next_page().await? {
            pages.push(page.contents);
        }
        pinned.close().await?;
        central
            .import_board_snapshot(pages.clone(), manifest.clone())
            .await?;
        let copy = central.copy_status(&source.selected.project).await?;
        let inputs = central.session_inputs()?;
        let receiver = inputs.receiver;
        let session_root = tempfile::tempdir()?;
        let session = ClaimSession::start(session_root.path(), inputs).await?;
        let claimed = session.claim(manifest.database).await;
        let stopped = session.shutdown().await;
        let grant = claimed?;
        stopped?;
        assert_eq!(grant.lease.epoch.value(), 2);
        assert_eq!(grant.start, manifest.position);
        let installed_path = source.path.with_file_name("e2.sqlite");
        let seeded = ReplicatedDatabase::seeding(
            &installed_path,
            BoardProjectSchema::new()?,
            grant,
            BOARD_LEASE_POLICY,
        )
        .await?;
        let staging_area = tempfile::tempdir()?;
        let staging = StagingStore::open(staging_area.path()).await?;
        let install = SnapshotInstall {
            install_id: InstallId::new(uuid::Uuid::now_v7())?,
            database: manifest.database,
            position: manifest.position,
            schema: manifest.schema.clone(),
        };
        staging.prepare(&install, &copy).await?;
        let mut cursor = None;
        for (index, page) in pages.into_iter().enumerate() {
            staging
                .stage_page(&install, u32::try_from(index)?, cursor.as_ref(), &page)
                .await?;
            cursor = page.next;
        }
        staging.verify(&install, &manifest).await?;
        staging.publish_writer(&install, &seeded).await?;
        seeded.mark_bootstrap_ready(&install, receiver).await?;
        seeded.migrate().await?;
        let mut installed = seeded.snapshot().await?;
        let installed_manifest = installed.manifest().await?;
        assert_eq!(installed_manifest.position, manifest.position);
        assert_eq!(installed_manifest.tables, manifest.tables);
        installed.close().await?;
        seeded.close().await;
        drop(seeded);
        let retained = fleet_project_writer::RetainedFile::open_read_only(
            &installed_path,
            BoardProjectSchema::new()?,
        )
        .await?;
        assert_eq!(retained.origin().epoch.value(), 2);
        drop(retained);
        // Act through the same offline API used for export and post-install readback.
        let mut exported = NativeProjectSnapshot::open(
            &mut offline,
            SelectedWriterFile {
                project: source.selected.project,
                peer: fixture.peer,
                file_epoch: Epoch::new(2)?,
            },
        )
        .await?;
        // Assert actual transport position, physical selection, A0 history and exact hashes.
        assert_eq!(exported.preflight().selected.file_epoch.value(), 2);
        assert_eq!(exported.preflight().manifest.position, manifest.position);
        assert_eq!(exported.preflight().application_epoch.get(), 1);
        assert_eq!(exported.preflight().high_water, source.cursor);
        assert_eq!(exported.preflight().latest_record_at, source.record_time);
        assert_eq!(exported.preflight().project_scope, source.scope);
        exported.verify_replicated_tables(&reframe_import_manifest(manifest.clone())?)?;
        let mut altered = reframe_import_manifest(manifest.clone())?;
        altered
            .tables
            .first_mut()
            .ok_or("source table manifest absent")?
            .digest = fleet_protocol::TableDigest::new("0".repeat(64))?;
        assert!(matches!(
            exported.verify_replicated_tables(&altered),
            Err(board_staging_cutover::NativeSourceError::ReplicatedTablesMismatch)
        ));
        let mut count = 0;
        while let Some(page) = exported.next_page().await? {
            assert_eq!(page.contents.position, manifest.position);
            count += 1;
        }
        assert!(count > 0);
        exported.close().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    result?;
    stopped?;
    Ok(())
}
