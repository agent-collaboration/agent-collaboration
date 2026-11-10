//! R13 for steps 0007 and 0008, and the picker list guard (review F3 and F4).
//!
//! After a full update, an older settings file, machine list or proxy store file that
//! comes back is stale: the guard refuses, the update lists it in a dry run, then removes
//! it unread without touching what it was carried into, and the guard clears. Back
//! without what it was carried into, it may hold the only copy, so the update stops and
//! names both. A root holding only the picker's list under its older name is refused
//! before a Host can create a Fleet database, and the update renames it for the picker.
use crate::update_ledger_journey::root_argument;
use crate::update_proxy_store_journey::seed_old_proxy_store;
use crate::update_settings_journey::write_old_settings;
use crate::update_support::{UpdateResult, populated_root, run_update, snapshot};
use agent_session_inventory::sessions::router_connection_registry::{
    RouterRegistryRead, read_router_registry,
};
use codex_router_host::{
    RequiredRouterUpdate, RouterUpdateRequirement, inspect_router_update_requirement,
    router_settings_path,
};
use codex_router_state::provider_proxy_database_path;
use std::path::{Path, PathBuf};

/// The picker's machine list with one machine, as an owner keeps it, comment included.
const MACHINE_LIST: &str = r#"{
  // the build box
  "version": 1,
  "routers": [{
    "name": "Sunbook",
    "connection": {"kind": "remote",
      "serviceId": "00000000-0000-4000-8000-000000000001",
      "mcpUrl": "https://machine.example.invalid:443/mcp",
      "nativeCodex": {"address": "wss://machine.example.invalid:443/"}},
    "defaultRemoteCwd": "/remote/only/project"
  }],
}
"#;

/// A root that has finished every step, with old settings and an old proxy store carried.
async fn finished_root() -> UpdateResult<(tempfile::TempDir, PathBuf)> {
    let (directory, root) = populated_root()?;
    write_old_settings(&root)?;
    seed_old_proxy_store(&root).await?;
    let update = run_update(&root_argument(&root)).await?;
    if update.status.code() != Some(0) {
        return Err(format!("{update:?}").into());
    }
    Ok((directory, root))
}

/// Bytes of every file under each of `paths`, so "untouched" is checked exactly.
fn destinations(paths: &[PathBuf]) -> UpdateResult<Vec<(PathBuf, String)>> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            found.extend(
                snapshot(path)?
                    .into_iter()
                    .map(|(file, digest)| (path.join(file), digest)),
            );
        } else {
            found.push((path.clone(), format!("{:?}", std::fs::read(path)?)));
        }
    }
    Ok(found)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn settings_and_proxy_files_back_after_their_steps_are_removed_unread() -> UpdateResult {
    // Arrange: a finished update, then each older file back, some of them malformed.
    let (_directory, root) = finished_root().await?;
    let carried_into = [
        router_settings_path(&root),
        root.join("agent-sessions.jsonc"),
        provider_proxy_database_path(&root),
        root.join("provider-proxy.sqlite.credential-locks"),
    ];
    let before = destinations(&carried_into)?;
    let locks = root.join("state.sqlite.credential-locks");
    std::fs::create_dir(&locks)?;
    std::fs::write(locks.join("old.lock"), b"held by an older proxy")?;
    let back: [(PathBuf, &[u8]); 5] = [
        (root.join("providers.json"), b"{this is not json"),
        (
            root.join("ports.json"),
            b"{\"router\": 1, \"mcp\": 2, \"tailscaleHttps\": 3}\n",
        ),
        (root.join("routers.jsonc"), b"not a machine list"),
        (root.join("state.sqlite"), b"this is not a database"),
        (root.join("state.sqlite-wal"), b"nor is this a log"),
    ];
    for (path, bytes) in &back {
        std::fs::write(path, bytes)?;
    }
    let all_back: Vec<&Path> = back
        .iter()
        .map(|(path, _)| path.as_path())
        .chain(std::iter::once(locks.as_path()))
        .collect();
    let root_before = snapshot(&root)?;

    // Act
    let refused = inspect_router_update_requirement(&root).await?;
    let dry_run = run_update(&[
        root_argument(&root)[0],
        root_argument(&root)[1],
        "--dry-run".as_ref(),
    ])
    .await?;
    let after_dry_run = snapshot(&root)?;
    let cleanup = run_update(&root_argument(&root)).await?;

    // Assert: refused, listed without change, removed, destinations byte for byte as
    // they were, and the Host may start.
    assert_eq!(
        refused,
        RouterUpdateRequirement::Required(RequiredRouterUpdate::LegacySettingsFile {
            path: root.join("providers.json")
        })
    );
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    let listed = String::from_utf8_lossy(&dry_run.stdout);
    for path in &all_back {
        let removal = format!("remove {} unread", path.display());
        assert!(
            listed
                .lines()
                .any(|line| line.starts_with("pending stale") && line.contains(&removal)),
            "the dry run must list {}: {listed}",
            path.display()
        );
    }
    assert_eq!(after_dry_run, root_before, "a dry run changes nothing");
    assert_eq!(
        cleanup.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&cleanup.stderr)
    );
    for path in &all_back {
        assert!(!path.exists(), "{} must be removed", path.display());
    }
    assert_eq!(
        destinations(&carried_into)?,
        before,
        "what they were carried into is untouched"
    );
    assert_eq!(
        inspect_router_update_requirement(&root).await?,
        RouterUpdateRequirement::NotRequired
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_proxy_store_back_without_the_renamed_store_stops_the_update_naming_both() -> UpdateResult
{
    // Arrange: a finished update, the renamed store gone, the former name back.
    let (_directory, root) = finished_root().await?;
    let current = provider_proxy_database_path(&root);
    std::fs::remove_file(&current)?;
    let former = root.join("state.sqlite");
    std::fs::write(&former, b"the only copy of these accounts")?;

    // Act
    let refused = run_update(&root_argument(&root)).await?;

    // Assert: stopped, naming both, the former store left as it was.
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "{} is back after its update step, but {}",
            former.display(),
            current.display()
        )),
        "{stderr}"
    );
    assert_eq!(std::fs::read(&former)?, b"the only copy of these accounts");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_root_with_only_the_older_machine_list_is_refused_then_renamed_for_the_picker()
-> UpdateResult {
    // Arrange: a root the picker used, with no Fleet database and no other old file.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    std::fs::create_dir(&root)?;
    let legacy = root.join("routers.jsonc");
    std::fs::write(&legacy, MACHINE_LIST)?;

    // Act
    let refused = inspect_router_update_requirement(&root).await?;
    let update = run_update(&root_argument(&root)).await?;

    // Assert: the Host is refused before it could create a Fleet database; the update
    // renames the list byte for byte; the picker reads the same machine from it.
    assert_eq!(
        refused,
        RouterUpdateRequirement::Required(RequiredRouterUpdate::LegacyMachineList {
            path: legacy.clone()
        })
    );
    assert_eq!(
        update.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert!(!legacy.exists());
    assert_eq!(
        std::fs::read_to_string(root.join("agent-sessions.jsonc"))?,
        MACHINE_LIST
    );
    let RouterRegistryRead::Ready(registry) = read_router_registry(&root) else {
        return Err(format!(
            "the picker must read the renamed list: {:?}",
            read_router_registry(&root)
        )
        .into());
    };
    assert_eq!(registry.routers.len(), 1);
    assert_eq!(registry.routers[0].name.as_str(), "Sunbook");
    assert_eq!(
        inspect_router_update_requirement(&root).await?,
        RouterUpdateRequirement::NotRequired
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn both_machine_lists_on_a_root_without_a_fleet_store_wait_for_the_owner_to_keep_one()
-> UpdateResult {
    // Arrange: the older list names a machine, a newer empty list sits beside it, and no
    // Fleet database exists for a Host to record step 0007 done in.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    std::fs::create_dir(&root)?;
    let legacy = root.join("routers.jsonc");
    std::fs::write(&legacy, MACHINE_LIST)?;
    let current = root.join("agent-sessions.jsonc");
    std::fs::write(&current, "{ \"version\": 1, \"routers\": [] }\n")?;
    let before = snapshot(&root)?;
    let refused_on = |path: &Path| {
        RouterUpdateRequirement::Required(RequiredRouterUpdate::LegacyMachineList {
            path: path.to_owned(),
        })
    };

    // Act and assert: the Host is refused; the update refuses, naming both; nothing
    // changes, and the older list keeps its bytes.
    assert_eq!(
        inspect_router_update_requirement(&root).await?,
        refused_on(&legacy)
    );
    let refused = run_update(&root_argument(&root)).await?;
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "both {} and {} exist",
            legacy.display(),
            current.display()
        )),
        "{stderr}"
    );
    assert_eq!(snapshot(&root)?, before, "a refused update changes nothing");

    // Act and assert: the owner keeps the older list; the Host is still refused until the
    // update carries it, then the picker's list holds those bytes and the Host may start.
    std::fs::remove_file(&current)?;
    assert_eq!(
        inspect_router_update_requirement(&root).await?,
        refused_on(&legacy)
    );
    assert_eq!(std::fs::read_to_string(&legacy)?, MACHINE_LIST);
    let carried = run_update(&root_argument(&root)).await?;
    assert_eq!(
        carried.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&carried.stderr)
    );
    assert!(!legacy.exists());
    assert_eq!(std::fs::read_to_string(&current)?, MACHINE_LIST);
    assert_eq!(
        inspect_router_update_requirement(&root).await?,
        RouterUpdateRequirement::NotRequired
    );
    Ok(())
}
