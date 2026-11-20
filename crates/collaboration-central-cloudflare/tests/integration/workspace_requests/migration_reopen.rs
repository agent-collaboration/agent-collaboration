//! Carry both SQLite-backed Durable Objects across separately disposed bridges.
use super::{support::*, workerd_host::WorkerdHost};
use central_storage::{ReadOutcome, Statement};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicI64},
};
use workspace_model as m;

const CAPTURE_ROOT: &str = "FLEET_WORKER_CAPTURE_ROOT";
const REOPEN_ROOT: &str = "FLEET_WORKER_REOPEN_ROOT";

#[derive(Serialize, Deserialize)]
struct ReopenWitness {
    workspace: m::WorkspaceId,
    area: m::AreaId,
    histories: [ReadOutcome; 2],
}

fn fixture_clock(root: &Path) -> Arc<HeldClock> {
    assert!(
        root.is_dir(),
        "the caller must own an existing persistence directory"
    );
    Arc::new(HeldClock(AtomicI64::new(FIXTURE_NOW * 1000)))
}

async fn migration_histories(
    host: &WorkerdHost,
    caller: &FixtureCaller,
    workspace: &m::WorkspaceId,
) -> [ReadOutcome; 2] {
    let runtime = host.central(&caller.organization_id).await;
    let history = || Statement {
        sql: "SELECT version,description,installed_on,success,hex(checksum) AS checksum,execution_time FROM _sqlx_migrations ORDER BY version",
        args: vec![],
    };
    [
        runtime.read(history()).await.unwrap(),
        runtime
            .inspect_workspace(workspace, history())
            .await
            .unwrap(),
    ]
}

async fn capture(root: &Path) -> ReopenWitness {
    let clock = fixture_clock(root);
    let host = WorkerdHost::open_with_persistence(clock.clone(), root)
        .await
        .unwrap();
    let caller = caller();
    seed(&host, &caller).await;
    let workspace = create_workspace(&host, &caller).await;
    let fixture = Fixture {
        host,
        caller,
        workspace,
        clock,
    };
    let area = fixture.area(14700).await;
    let histories = migration_histories(&fixture.host, &fixture.caller, &fixture.workspace).await;
    fixture.host.close().await;
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
    let temporary = tempfile::Builder::new()
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .tempdir()
        .unwrap();
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

    let host = WorkerdHost::open_with_persistence(fixture_clock(&root), &root)
        .await
        .unwrap();
    let caller = caller();
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
    assert_eq!(
        migration_histories(&host, &caller, &witness.workspace).await,
        witness.histories
    );
    host.close().await;
}
