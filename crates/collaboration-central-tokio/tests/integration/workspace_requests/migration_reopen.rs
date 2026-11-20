//! Carry a closed central and workspace store across builds of the owning host.
use super::support::*;
use central_storage::ReadOutcome;
use collaboration_central_core::storage_conformance;
use collaboration_central_tokio::{ObjectStorage, TokioWorkspaceHost};
use fleet_storage_schema::workspace_migrations;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, path::PathBuf, sync::Arc};
use workspace_model as m;

const CAPTURE_ROOT: &str = "FLEET_NATIVE_CAPTURE_ROOT";
const REOPEN_ROOT: &str = "FLEET_NATIVE_REOPEN_ROOT";
const FIXTURE_TIME_MS: i64 = 1_794_571_200_000;

#[derive(Serialize, Deserialize)]
struct ReopenWitness {
    workspace: m::WorkspaceId,
    area: m::AreaId,
    histories: [ReadOutcome; 2],
}

fn database_paths(root: &Path, workspace: &m::WorkspaceId) -> [PathBuf; 2] {
    [
        root.join("central")
            .join(format!("{}.sqlite", organization().as_str())),
        root.join("workspace-stores")
            .join(format!("{workspace}.sqlite")),
    ]
}

async fn migration_histories(root: &Path, workspace: &m::WorkspaceId) -> [ReadOutcome; 2] {
    let [central, workspace] = database_paths(root, workspace);
    let mut histories = Vec::new();
    for (path, migrator) in [
        (central, workspace_migrations::central_migrator()),
        (workspace, workspace_migrations::workspace_migrator()),
    ] {
        assert!(
            path.is_file(),
            "the owning request must have opened this store"
        );
        let store = ObjectStorage::new(path, Arc::new(migrator));
        let history = store
            .read(storage_conformance::list_applied_migrations())
            .await
            .unwrap();
        store.close().await;
        histories.push(history);
    }
    histories.try_into().unwrap()
}

async fn capture(root: &Path) -> ReopenWitness {
    let fixture = Fixture::new().await;
    let area = fixture.area(14700).await;
    fixture.host.close().await;
    let histories = migration_histories(fixture.root.path(), &fixture.workspace).await;
    for (source, destination) in database_paths(fixture.root.path(), &fixture.workspace)
        .into_iter()
        .zip(database_paths(root, &fixture.workspace))
    {
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
    let witness = ReopenWitness {
        workspace: fixture.workspace,
        area,
        histories,
    };
    fs::write(
        root.join("witness.json"),
        serde_json::to_vec(&witness).unwrap(),
    )
    .unwrap();
    witness
}

#[tokio::test]
async fn the_owning_host_reopens_stored_state_with_zero_applied_migrations() {
    let temporary = tempfile::tempdir().unwrap();
    let capture_root = std::env::var_os(CAPTURE_ROOT);
    let reopen_root = std::env::var_os(REOPEN_ROOT);
    assert!(capture_root.is_none() || reopen_root.is_none());
    let reopen_existing = reopen_root.is_some();
    let root = capture_root
        .or(reopen_root)
        .map_or_else(|| temporary.path().to_owned(), PathBuf::from);
    let witness = if reopen_existing {
        serde_json::from_slice(&fs::read(root.join("witness.json")).unwrap()).unwrap()
    } else {
        capture(&root).await
    };
    for (history, rows) in witness.histories.iter().zip([5, 1]) {
        assert_eq!(history.rows.len(), rows);
        assert_eq!(history.columns.len(), 6);
    }

    let clock = Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            FIXTURE_TIME_MS,
        ))
        .unwrap(),
    );
    let keys = signed::SignInKeys::start().await.unwrap();
    let host = TokioWorkspaceHost::open_with_signin(&root, clock, settings(&keys.url))
        .await
        .unwrap();
    let caller = authenticated(
        &host,
        m::Caller::Session(workspace_ids::SessionRef {
            machine_id: id(2),
            session_id: "test-session".to_owned().try_into().unwrap(),
        }),
        &id::<m::WorkspaceId>(4).to_string(),
        FIXTURE_TIME_MS / 1000,
    )
    .await;
    let reply = call(
        &host,
        &caller,
        Some(witness.workspace.clone()),
        14700,
        m::WorkspaceOperation::Show {
            item: m::ItemRef::Area(witness.area.clone()),
        },
    )
    .await;
    assert!(matches!(
        reply.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { .. })
    ));
    let replay = call(
        &host,
        &caller,
        Some(witness.workspace.clone()),
        14700,
        m::WorkspaceOperation::CreateArea {
            name: name("Area 14700"),
            description: "Test description".to_owned().try_into().unwrap(),
        },
    )
    .await;
    assert_eq!(created(replay), m::ItemRef::Area(witness.area));
    host.close().await;
    assert_eq!(
        migration_histories(&root, &witness.workspace).await,
        witness.histories
    );
}
