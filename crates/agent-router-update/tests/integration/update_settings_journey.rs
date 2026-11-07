//! J1 (step part) and J2 (rename): step 0007 on the populated root. The old release's
//! `providers.json` and `ports.json` end up unchanged as the sections of
//! `settings.jsonc`, read back through the Host's own readers; `routers.jsonc` becomes
//! `agent-sessions.jsonc` byte for byte; a kill at every step barrier reruns to the same
//! end; a `settings.jsonc` the update didn't write, or a malformed old file, stops the
//! update before it changes anything; and the Host refuses a root with an old settings
//! file even when its Fleet database was never opened.
use crate::update_ledger_journey::root_argument;
use crate::update_support::{
    UpdateResult, populated_root, run_update, run_update_killed_at, snapshot,
};
use codex_router_host::{
    HostPortOverrides, RequiredRouterUpdate, RouterUpdateRequirement,
    inspect_router_update_requirement, load_or_create_provider_settings, resolve_host_ports,
    router_fleet_database, router_settings_path,
};
use message_board_storage::{RouterUpdateStep, UpdateSourceProgress, read_update_ledger};
use std::path::{Path, PathBuf};

/// `providers.json` as the old release's provider configuration writer left it (pretty
/// JSON and a trailing newline), with Claude disabled and given its own executable.
const OLD_PROVIDERS: &str = "{\n  \"version\": 1,\n  \"providers\": {\n    \"claude\": {\n      \"enabled\": false,\n      \"executable\": \"/opt/claude/claude-agent-acp\",\n      \"arguments\": []\n    },\n    \"cursor\": {\n      \"enabled\": true,\n      \"executable\": null,\n      \"arguments\": [\n        \"acp\"\n      ]\n    }\n  }\n}\n";
/// `ports.json` as an owner wrote it by hand.
const OLD_PORTS: &str = "{\"router\": 28741, \"mcp\": 28742, \"tailscaleHttps\": 28743}\n";
/// `routers.jsonc` with an owner's comment, carried byte for byte.
const OLD_MACHINE_LIST: &str =
    "{\n  // machines I open sessions on\n  \"version\": 1,\n  \"routers\": [],\n}\n";

struct OldSettings {
    providers: PathBuf,
    ports: PathBuf,
    machine_list: PathBuf,
}

fn write_old_settings(root: &Path) -> UpdateResult<OldSettings> {
    let old = OldSettings {
        providers: root.join("providers.json"),
        ports: root.join("ports.json"),
        machine_list: root.join("routers.jsonc"),
    };
    std::fs::write(&old.providers, OLD_PROVIDERS)?;
    std::fs::write(&old.ports, OLD_PORTS)?;
    std::fs::write(&old.machine_list, OLD_MACHINE_LIST)?;
    Ok(old)
}

/// After a finished step 0007: values through the Host's readers, the files left, the
/// ledger and the guard.
async fn assert_settings_carried(root: &Path, old: &OldSettings, context: &str) -> UpdateResult {
    let settings = std::fs::read_to_string(router_settings_path(root))?;
    for (file, section) in [(OLD_PROVIDERS, "providers"), (OLD_PORTS, "ports")] {
        assert!(
            settings.contains(&format!("\"{section}\": {}", file.trim())),
            "{context}: the {section} section must hold the old file unchanged:\n{settings}"
        );
    }
    let providers = load_or_create_provider_settings(root)?;
    assert!(!providers.claude().enabled, "{context}");
    assert_eq!(
        providers.claude().executable,
        Some(PathBuf::from("/opt/claude/claude-agent-acp")),
        "{context}"
    );
    assert_eq!(providers.cursor().arguments, vec!["acp"], "{context}");
    let ports = resolve_host_ports(root, false, HostPortOverrides::default())?;
    assert_eq!(
        (
            ports.selection().router.port,
            ports.selection().mcp.port,
            ports.tailscale_https_port().get()
        ),
        (28741, 28742, 28743),
        "{context}"
    );
    for gone in [&old.providers, &old.ports, &old.machine_list] {
        assert!(!gone.exists(), "{context}: {} must be gone", gone.display());
    }
    assert_eq!(
        std::fs::read_to_string(root.join("agent-sessions.jsonc"))?,
        OLD_MACHINE_LIST,
        "{context}: the machine list keeps its bytes under its new name"
    );
    let ledger = read_update_ledger(&router_fleet_database(root)).await?;
    let step = RouterUpdateStep::CarrySettingsFiles.number();
    assert!(ledger.step_done(step), "{context}: {ledger:?}");
    assert_eq!(
        ledger.source_progress(step, "settings.jsonc"),
        Some(UpdateSourceProgress::Removed),
        "{context}"
    );
    assert_eq!(
        inspect_router_update_requirement(root).await?,
        RouterUpdateRequirement::NotRequired,
        "{context}"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn the_old_settings_files_become_settings_jsonc_and_the_machine_list_is_renamed()
-> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let old = write_old_settings(&root)?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8_lossy(&output.stdout);
    let expected_line = format!(
        "done 0007: write {} from {} and {}, then remove them; rename {} to {}\n",
        router_settings_path(&root).display(),
        old.providers.display(),
        old.ports.display(),
        old.machine_list.display(),
        root.join("agent-sessions.jsonc").display()
    );
    assert!(report.contains(&expected_line), "{report}");
    assert_settings_carried(&root, &old, "full update").await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_kill_at_every_step_0007_barrier_reruns_to_the_same_carried_settings() -> UpdateResult {
    let markers = [
        "step-0007:settings-written",
        "step-0007:recorded-written",
        "step-0007:old-file-removed:providers.json",
        "step-0007:old-file-removed:ports.json",
        "step-0007:machine-list-renamed",
        "step-0007:done",
    ];
    for marker in markers {
        // Arrange
        let (_directory, root) = populated_root()?;
        let old = write_old_settings(&root)?;

        // Act
        run_update_killed_at(&root, marker).await?;
        let refused = inspect_router_update_requirement(&root).await?;
        let rerun = run_update(&root_argument(&root)).await?;

        // Assert: the Host refuses the interrupted root, and the rerun finishes it.
        assert!(
            matches!(refused, RouterUpdateRequirement::Required(_)),
            "killed at {marker}: guard said {refused:?}"
        );
        assert_eq!(
            rerun.status.code(),
            Some(0),
            "killed at {marker}: {}",
            String::from_utf8_lossy(&rerun.stderr)
        );
        assert_settings_carried(&root, &old, &format!("killed at {marker}")).await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_settings_file_the_update_did_not_write_beside_an_old_one_stops_the_update_naming_both()
-> UpdateResult {
    // Arrange: the owner already made a settings.jsonc, and providers.json is still here.
    let (_directory, root) = populated_root()?;
    let old = write_old_settings(&root)?;
    let settings = router_settings_path(&root);
    std::fs::write(&settings, "{ \"ports\": { \"router\": 29741 } }\n")?;
    let before = snapshot(&root)?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert_ne!(output.status.code(), Some(0));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(&settings.display().to_string()), "{error}");
    assert!(
        error.contains(&old.providers.display().to_string()),
        "{error}"
    );
    assert_eq!(snapshot(&root)?, before, "a refused update changes nothing");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_malformed_old_settings_file_stops_the_update_naming_it() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let old = write_old_settings(&root)?;
    std::fs::write(&old.ports, "{\"router\": 28741,,}\n")?;
    let before = snapshot(&root)?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert_ne!(output.status.code(), Some(0));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(&old.ports.display().to_string()), "{error}");
    assert_eq!(snapshot(&root)?, before, "a refused update changes nothing");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn both_machine_list_names_are_left_for_the_picker_to_name() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let old = write_old_settings(&root)?;
    let current = root.join("agent-sessions.jsonc");
    let current_list = "{ \"version\": 1, \"routers\": [] }\n";
    std::fs::write(&current, current_list)?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&old.machine_list)?,
        OLD_MACHINE_LIST
    );
    assert_eq!(std::fs::read_to_string(&current)?, current_list);
    assert!(!old.providers.exists() && !old.ports.exists());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_root_that_never_opened_its_fleet_database_but_has_old_settings_is_refused_until_updated()
-> UpdateResult {
    // Arrange: no Fleet database at all, only the old settings files.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    std::fs::create_dir(&root)?;
    let old = write_old_settings(&root)?;

    // Act
    let before = inspect_router_update_requirement(&root).await?;
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        before,
        RouterUpdateRequirement::Required(RequiredRouterUpdate::LegacySettingsFile {
            path: old.providers.clone()
        })
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_settings_carried(&root, &old, "never-connected root").await
}
