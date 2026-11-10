//! J0 and J4 together: every PR A step, in order, on one root holding everything an
//! older Router leaves: the populated Fleet stores, the old settings files, the old
//! provider proxy store with rows only in its log, the dead files with approval routes,
//! an unfinished enrollment reset, and the machine's identity and enrollment files,
//! which PR A leaves for PR B. A full update, and a kill at every barrier of
//! every step followed by a rerun, both end at the same root: each step's values read
//! back through their owners, the identity and enrollment files keep their bytes, and
//! the root holds exactly the files listed below.
use crate::update_ledger_journey::root_argument;
use crate::update_moves_journey::fixture_values;
use crate::update_proxy_store_journey::{
    ProxyValues, assert_proxy_store_renamed, seed_old_proxy_store,
};
use crate::update_reset_and_dead_files::{
    ROUTE_THREAD, imported_route_threads, names_in, write_dead_files, write_reset_journal,
};
use crate::update_settings_journey::{OldSettings, assert_settings_carried, write_old_settings};
use crate::update_support::{
    FleetValues, UpdateResult, populated_root, run_update, run_update_killed_at, snapshot,
};
use codex_router_host::{
    RouterUpdateRequirement, inspect_router_update_requirement, router_collaboration_directory,
    router_fleet_database,
};
use message_board_storage::{RouterUpdateStep, read_update_ledger};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Every barrier of every step, in the order the update reaches them.
const EVERY_BARRIER: [&str; 26] = [
    "step-0001:migrated",
    "step-0001:done",
    "step-0002:done",
    "step-0005:fleet-outbox.sqlite:written",
    "step-0005:fleet-outbox.sqlite:verified",
    "step-0005:fleet-outbox.sqlite:database-removed",
    "step-0005:fleet-outbox.sqlite:removed",
    "step-0005:board-cursors.sqlite:written",
    "step-0005:board-cursors.sqlite:verified",
    "step-0005:board-cursors.sqlite:database-removed",
    "step-0005:board-cursors.sqlite:removed",
    "step-0005:done",
    "step-0007:settings-written",
    "step-0007:recorded-written",
    "step-0007:old-file-removed:providers.json",
    "step-0007:old-file-removed:ports.json",
    "step-0007:machine-list-renamed",
    "step-0007:done",
    "step-0008:checkpointed",
    "step-0008:store-renamed",
    "step-0008:lock-folder-renamed",
    "step-0008:done",
    "step-0009:done",
    "step-0010:before-checkpoint",
    "step-0010:checkpointed",
    "step-0010:renamed",
];

/// What the old root held, and so what the updated root must show.
struct OldRoot {
    fleet: FleetValues,
    settings: OldSettings,
    proxy: ProxyValues,
    reset_journal: PathBuf,
    session_state: PathBuf,
    /// Files PR A must not touch, with the bytes the older Router left in them.
    kept_for_pr_b: Vec<(PathBuf, Vec<u8>)>,
}

/// The machine's identity and enrollment files as an older Router wrote them, owner-only
/// as its writers make them. PR B's step 0003 moves them; until then no PR A step reads,
/// rewrites, removes or loosens them. Their contents are opaque here: the compiled-CLI
/// journey proves the Host's own readers accept a genuinely enrolled root.
fn write_identity_and_enrollment(root: &Path) -> UpdateResult<Vec<(PathBuf, Vec<u8>)>> {
    let collaboration = router_collaboration_directory(root);
    let fleet = root.join("fleet");
    std::fs::DirBuilder::new().mode(0o700).create(&fleet)?;
    let files = [
        (
            collaboration.join("service-identity.json"),
            br#"{"version":1,"serviceId":"7c6c8a4e-3f1b-4b6a-9d2e-5a1f0c3b8e71"}"#.to_vec(),
        ),
        (
            collaboration.join("installation-peer.json"),
            br#"{"version":1,"peerId":"019f0000-0000-7000-8000-00000000d3f1"}"#.to_vec(),
        ),
        (
            fleet.join("enrollment.json"),
            br#"{"endpoint":"https://fleet.example.test","access":{"clientId":"d3-access-client","clientSecretKey":"fleet-access-client-secret"},"credentialKey":"fleet-machine-credential","peer":"019f0000-0000-7000-8000-00000000d3f1","receiver":"019f0000-0000-7000-8000-00000000d3f2","relay":"019f0000-0000-7000-8000-00000000d3f3","expiresAt":null}"#.to_vec(),
        ),
    ];
    for (path, bytes) in &files {
        std::fs::write(path, bytes)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(files.into())
}

/// One root with everything an older Router leaves that PR A's steps handle.
async fn whole_old_root(root: &Path) -> UpdateResult<OldRoot> {
    let mut fleet = fixture_values(root).await?;
    // The unfinished reset clears the Fleet cache before anything moves.
    fleet.cached_records.clear();
    fleet.checkpoint = 0;
    let settings = write_old_settings(root)?;
    let proxy = seed_old_proxy_store(root).await?;
    write_dead_files(root)?;
    let reset_journal = write_reset_journal(root, "pending")?;
    let collaboration = router_collaboration_directory(root);
    let session_state = collaboration.join("machine-session/state.json");
    std::fs::create_dir_all(collaboration.join("machine-session"))?;
    std::fs::write(&session_state, b"old session cursor")?;
    let kept_for_pr_b = write_identity_and_enrollment(root)?;
    Ok(OldRoot {
        fleet,
        settings,
        proxy,
        reset_journal,
        session_state,
        kept_for_pr_b,
    })
}

/// The updated root: exactly these files, then every step's values through their
/// owners, every step done, and the Host allowed to start.
async fn assert_whole_update_finished(root: &Path, old: &OldRoot, context: &str) -> UpdateResult {
    // Listed before any check opens a store, since an open adds its own log files.
    // The Router root: the update's Host lock, the carried settings and machine list,
    // the renamed proxy store with its lock folder, the enrollment folder PR B moves,
    // and the collaboration directory.
    assert_eq!(
        names_in(root)?,
        [
            "agent-communication",
            "agent-sessions.jsonc",
            "fleet",
            "host.lock",
            "provider-proxy.sqlite",
            "provider-proxy.sqlite.credential-locks",
            "settings.jsonc",
        ],
        "{context}: root listing"
    );
    // The collaboration directory: the reset's lock file, which the Host also takes at
    // every start; the Fleet database and the store the routes moved into, each with
    // the log Turso leaves on close; the two identity files PR B moves; and the session
    // folder the reset emptied. No older store, no other JSON file, no dead file is left.
    assert_eq!(
        names_in(&router_collaboration_directory(root))?,
        [
            "enrollment-reset.lock",
            "fleet.sqlite",
            "fleet.sqlite-wal",
            "installation-peer.json",
            "interaction.sqlite",
            "interaction.sqlite-wal",
            "machine-session",
            "service-identity.json",
        ],
        "{context}: collaboration listing"
    );
    assert_eq!(
        &FleetValues::read_merged(&router_fleet_database(root)).await?,
        &old.fleet,
        "{context}: Fleet values carried, the cache cleared by the reset"
    );
    assert_settings_carried(root, &old.settings, context).await?;
    assert_proxy_store_renamed(root, &old.proxy, context).await?;
    assert_eq!(
        imported_route_threads(root).await?,
        vec![ROUTE_THREAD],
        "{context}: approval routes imported"
    );
    assert_eq!(
        names_in(&root.join("fleet"))?,
        ["enrollment.json"],
        "{context}"
    );
    let mode = |path: &Path| -> std::io::Result<u32> {
        Ok(std::fs::symlink_metadata(path)?.permissions().mode() & 0o777)
    };
    assert_eq!(mode(&root.join("fleet"))?, 0o700, "{context}: fleet folder");
    for (path, bytes) in &old.kept_for_pr_b {
        assert_eq!(
            &std::fs::read(path)?,
            bytes,
            "{context}: {} must keep its bytes until PR B",
            path.display()
        );
        assert_eq!(
            mode(path)?,
            0o600,
            "{context}: {} must stay owner-only until PR B",
            path.display()
        );
    }
    assert!(!old.reset_journal.exists(), "{context}: reset journal");
    assert!(!old.session_state.exists(), "{context}: session cursor");
    let ledger = read_update_ledger(&router_fleet_database(root)).await?;
    for step in RouterUpdateStep::KNOWN {
        assert!(
            ledger.step_done(step.number()),
            "{context}: step {} not done: {ledger:?}",
            step.label()
        );
    }
    assert_eq!(
        inspect_router_update_requirement(root).await?,
        RouterUpdateRequirement::NotRequired,
        "{context}: guard"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_full_update_runs_every_step_in_order_and_leaves_exactly_the_updated_root() -> UpdateResult
{
    // Arrange
    let (_directory, root) = populated_root()?;
    let old = whole_old_root(&root).await?;
    let before = snapshot(&root)?;

    // Act: a dry run, then the update.
    let dry_run = run_update(&[
        root_argument(&root)[0],
        root_argument(&root)[1],
        "--dry-run".as_ref(),
    ])
    .await?;
    let after_dry_run = snapshot(&root)?;
    let update = run_update(&root_argument(&root)).await?;

    // Assert: the dry run lists every step in catalog order and changes nothing; the
    // update runs them in that order and leaves exactly the updated root.
    let pending_steps = |stdout: &[u8], verb: &str| {
        String::from_utf8_lossy(stdout)
            .lines()
            .filter_map(|line| line.strip_prefix(&format!("{verb} ")))
            .filter_map(|rest| rest.split_once(':').map(|(label, _)| label.to_owned()))
            .collect::<Vec<_>>()
    };
    // PR A's steps in the order the design gives them, not read from the catalog.
    let catalog = ["0001", "0002", "0005", "0007", "0008", "0009", "0010"];
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    assert_eq!(pending_steps(&dry_run.stdout, "pending"), catalog);
    assert_eq!(after_dry_run, before, "a dry run changes nothing");
    assert_eq!(
        update.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert_eq!(pending_steps(&update.stdout, "done"), catalog);
    assert_whole_update_finished(&root, &old, "full update").await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_kill_at_every_barrier_of_every_step_reruns_to_the_same_updated_root() -> UpdateResult {
    for marker in EVERY_BARRIER {
        // Arrange
        let (_directory, root) = populated_root()?;
        let old = whole_old_root(&root).await?;

        // Act
        run_update_killed_at(&root, marker).await?;
        let refused = inspect_router_update_requirement(&root).await?;
        let rerun = run_update(&root_argument(&root)).await?;

        // Assert: the Host stays refused until the update finishes; the rerun ends at
        // the same root as an uninterrupted update.
        assert_ne!(
            refused,
            RouterUpdateRequirement::NotRequired,
            "killed at {marker}: the Host must not start yet"
        );
        assert_eq!(
            rerun.status.code(),
            Some(0),
            "killed at {marker}: {}",
            String::from_utf8_lossy(&rerun.stderr)
        );
        assert_whole_update_finished(&root, &old, &format!("killed at {marker}")).await?;
    }
    Ok(())
}
