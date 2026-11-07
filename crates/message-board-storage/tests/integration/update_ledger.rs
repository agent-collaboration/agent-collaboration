//! The update ledger inside the Fleet database: opened only where it exists, read
//! without the store's lock, and one row per step however often it is recorded.

use message_board_storage::{
    FleetCache, RouterUpdateStep, StoredUpdateLedger, UpdateLedger, UpdateLedgerEntry,
    UpdateStepNumber, UpdateStepProgress, read_update_ledger,
};
use sqlx::{Connection, Executor};
use sqlx_turso::{TursoConnectOptions, TursoConnection};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn step(number: u16) -> UpdateStepNumber {
    UpdateStepNumber::new(number)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn migrating_an_existing_store_never_creates_a_missing_one() -> TestResult {
    // Arrange
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("fleet-cache.sqlite");

    // Act
    let opened = UpdateLedger::migrate_existing(&missing).await;

    // Assert
    assert!(
        opened.is_err(),
        "a missing Fleet database must not be opened"
    );
    assert!(
        !missing.exists(),
        "the refused open must not create the file"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn reading_the_ledger_neither_creates_nor_migrates_the_store() -> TestResult {
    // Arrange: one root with no store, one store an older Router made without the ledger.
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.sqlite");
    let older = directory.path().join("older.sqlite");
    let mut connection = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&older)
            .create_if_missing(true),
    )
    .await?;
    connection
        .execute("CREATE TABLE cached_fleet_records(kind TEXT, id TEXT, payload TEXT)")
        .await?;
    connection.close().await?;

    // Act
    let missing_ledger = read_update_ledger(&missing).await?;
    let older_ledger = read_update_ledger(&older).await?;

    // Assert
    assert_eq!(missing_ledger, StoredUpdateLedger::MissingDatabase);
    assert!(!missing.exists(), "reading must not create the store");
    assert_eq!(older_ledger, StoredUpdateLedger::WithoutLedger);
    assert_eq!(
        read_update_ledger(&older).await?,
        StoredUpdateLedger::WithoutLedger,
        "reading must not have migrated the ledger table in"
    );
    Ok(())
}

const HELD_STORE_VARIABLE: &str = "UPDATE_LEDGER_HELD_STORE";
const HELD_SIGNAL: &str = "update-ledger-writer-holds-the-store";

/// Child process: holds the store's writable connection, as a running Host does,
/// until its parent closes stdin.
#[tokio::test]
#[ignore = "launched as a separate writer process by its parent test"]
async fn ledger_writer_child() -> TestResult {
    let path = std::path::PathBuf::from(
        std::env::var_os(HELD_STORE_VARIABLE).ok_or("run only by the parent test")?,
    );
    let writer = UpdateLedger::create(&path).await?;
    writer
        .record_step(step(1), UpdateStepProgress::InProgress)
        .await?;
    writer
        .record_step(step(1), UpdateStepProgress::Done)
        .await?;
    println!("{HELD_SIGNAL}");
    tokio::task::spawn_blocking(|| std::io::stdin().read_line(&mut String::new())).await??;
    writer.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn the_ledger_reads_while_another_process_holds_the_store_and_keeps_one_row_per_step()
-> TestResult {
    // Arrange: a separate process holds the store writable, as a running Host does;
    // the store's lock only refuses opens from other processes.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet-cache.sqlite");
    let child_namespace = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, namespace)| namespace);
    let mut writer = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            &format!("{child_namespace}::ledger_writer_child"),
            "--ignored",
            "--nocapture",
        ])
        .env(HELD_STORE_VARIABLE, &path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut lines = tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(
        writer.stdout.take().ok_or("child stdout")?,
    ));
    let held = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await? {
            if line == HELD_SIGNAL {
                return Ok::<bool, std::io::Error>(true);
            }
        }
        Ok(false)
    })
    .await??;
    assert!(
        held,
        "the writer child never ran; its selector matched nothing"
    );

    // Act
    let read_while_held = read_update_ledger(&path).await;
    drop(writer.stdin.take());
    let child = tokio::time::timeout(std::time::Duration::from_secs(30), writer.wait()).await??;

    // Assert
    let expected = vec![UpdateLedgerEntry::Step {
        step: step(1),
        progress: UpdateStepProgress::Done,
    }];
    assert_eq!(
        read_while_held?,
        StoredUpdateLedger::Present(expected.clone())
    );
    assert!(child.success(), "the writer child failed");
    let reread = read_update_ledger(&path).await?;
    assert!(reread.step_done(step(1)));
    assert!(!reread.step_done(step(5)));
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_fleet_store_the_host_creates_records_every_step_and_an_existing_one_gains_none()
-> TestResult {
    // Arrange: no store at one path; at the other, a store an older Router made.
    let directory = tempfile::tempdir()?;
    let created = directory.path().join("created.sqlite");
    let older = directory.path().join("older.sqlite");
    let mut connection = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&older)
            .create_if_missing(true),
    )
    .await?;
    connection
        .execute("CREATE TABLE unrelated_older_table(value TEXT)")
        .await?;
    connection.close().await?;

    // Act: the Host's own open, once creating and once migrating an existing store.
    FleetCache::open(&created).await?.close().await;
    FleetCache::open(&older).await?.close().await;

    // Assert: a store with nothing old needs no step; an existing store gets the empty
    // ledger table only, so the guard keeps treating it as unfinished.
    let every_step_done = RouterUpdateStep::KNOWN
        .map(|step| UpdateLedgerEntry::Step {
            step: step.number(),
            progress: UpdateStepProgress::Done,
        })
        .to_vec();
    assert_eq!(
        read_update_ledger(&created).await?,
        StoredUpdateLedger::Present(every_step_done)
    );
    assert_eq!(
        read_update_ledger(&older).await?,
        StoredUpdateLedger::Present(vec![])
    );
    Ok(())
}
