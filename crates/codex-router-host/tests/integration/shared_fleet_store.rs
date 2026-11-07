//! J8 at the Host: the composed runtime keeps one Fleet database, `fleet.sqlite`, with no
//! older store beside it; a second process asking to write it gets the store's lock
//! error while the runtime runs; and the runtime then shuts down, restarts on the same
//! store and keeps the board cursor key it installed.
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use message_board_storage::{FleetStore, RouterUpdateStep, StoredUpdateLedger, read_update_ledger};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

type StoreResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const SECOND_WRITER_STORE: &str = "SHARED_FLEET_STORE_SECOND_WRITER";
const SECOND_WRITER_LOCKED: &str = "second-writer-refused-by-the-store-lock";
const SECOND_WRITER_OPENED: &str = "second-writer-opened-the-store";

fn inputs(root: &Path) -> CollaborationRuntimeInputs {
    CollaborationRuntimeInputs {
        board_session: None,
        directory: root.to_owned(),
        codex_home: root.to_owned(),
        backend_socket: root.join("backend.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: None,
        remote_control_server_name: None,
        owner_human_id: None,
    }
}

async fn installed_cursor_key(store: &Path) -> StoreResult<Vec<u8>> {
    let fleet = FleetStore::open(store).await?;
    let key = fleet
        .cursors()
        .key()
        .await?
        .ok_or("no board cursor key installed")?;
    fleet.close().await;
    Ok(key.as_bytes().to_vec())
}

/// Child process: asks for a writable open of the running Host's Fleet database.
#[tokio::test]
#[ignore = "launched as a separate process by its parent test"]
async fn second_writer_child() -> StoreResult {
    let path = PathBuf::from(std::env::var_os(SECOND_WRITER_STORE).ok_or("parent only")?);
    match FleetStore::open(&path).await {
        Ok(store) => {
            store.close().await;
            println!("{SECOND_WRITER_OPENED}");
        }
        Err(error) if error.to_string().contains("locked by another process") => {
            println!("{SECOND_WRITER_LOCKED}");
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn the_running_host_holds_one_fleet_database_a_second_writer_cannot_open() -> StoreResult {
    // Arrange
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let store = root.path().join(FleetStore::FILE_NAME);
    let child_namespace = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, namespace)| namespace);

    // Act: start the Host's runtime, then ask a second process to write its store.
    let runtime = CollaborationRuntime::start(inputs(root.path())).await?;
    let second_writer = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                &format!("{child_namespace}::second_writer_child"),
                "--ignored",
                "--nocapture",
            ])
            .env(SECOND_WRITER_STORE, &store)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    let ledger_while_running = read_update_ledger(&store).await?;
    runtime.shutdown().await?;
    let first_key = installed_cursor_key(&store).await?;
    let restarted = CollaborationRuntime::start(inputs(root.path())).await?;
    restarted.shutdown().await?;
    let key_after_restart = installed_cursor_key(&store).await?;

    // Assert
    let child_stdout = String::from_utf8_lossy(&second_writer.stdout);
    assert!(
        child_stdout.contains(SECOND_WRITER_LOCKED),
        "the second writer must be refused by the store lock (child said: {child_stdout}{})",
        String::from_utf8_lossy(&second_writer.stderr)
    );
    let StoredUpdateLedger::Present(entries) = ledger_while_running else {
        return Err("the Host's Fleet database carries no ledger".into());
    };
    assert_eq!(
        entries.len(),
        RouterUpdateStep::KNOWN.len(),
        "a new store records every step"
    );
    for older in [
        "fleet-cache.sqlite",
        "fleet-outbox.sqlite",
        "board-cursors.sqlite",
    ] {
        assert!(!root.path().join(older).exists(), "{older} must not exist");
    }
    assert_eq!(first_key.len(), 32);
    assert_eq!(
        key_after_restart, first_key,
        "a restart keeps the installed cursor key"
    );
    Ok(())
}
