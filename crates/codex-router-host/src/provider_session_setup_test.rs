//! A session's setup rebuilt from its record (D1 R18; owner Decision 3): as
//! its create built it, with the binding's roots as configured now, and
//! nothing for a record written before the resume inputs were kept.

use super::{BindingSessionSetup, restore_setup};
use crate::provider_work_roots::{AskOutsideHook, ProviderWorkRoots, WorkRootsHook};
use collaboration_protocol::ProviderWorkingDirectory;
use collaboration_runtime::ProviderSessionResumeInputs;
use std::path::{Path, PathBuf};

fn folder(path: &Path) -> ProviderWorkingDirectory {
    ProviderWorkingDirectory::try_from(path.display().to_string()).expect("absolute folder")
}

fn configured(root: &Path) -> ProviderWorkRoots {
    ProviderWorkRoots {
        roots: vec![root.join("configured")],
        hook: WorkRootsHook::Ready(AskOutsideHook {
            node: PathBuf::from("/usr/local/bin/node"),
            hook: root.join("hooks/router-work-roots-hook.ts"),
        }),
    }
}

/// Oracle: owner Decision 3, "old rows keep the fallback": a record without a
/// requested mode rebuilds nothing (no folders, no Router auto), so the
/// restored session asks for everything, as before.
#[test]
fn a_record_from_before_the_resume_inputs_rebuilds_nothing() {
    // Arrange
    let root = tempfile::tempdir().expect("root");

    // Act
    let setup = restore_setup(
        &configured(root.path()),
        root.path(),
        &ProviderSessionResumeInputs::default(),
    );

    // Assert
    assert_eq!(setup, acp_client_runtime::SessionRestoreSetup::default());
}

/// Oracle: D1 R18, a session in auto works in the configured roots, its
/// project's tmp/ and the roots its creator named, and Claude's hook asks
/// outside every folder it works in, its scratch folder included.
#[test]
fn an_auto_record_rebuilds_the_configured_and_named_roots_with_its_scratch_folder() {
    // Arrange
    let root = tempfile::tempdir().expect("root");
    let named = root.path().join("named");
    let scratch = root.path().join("scratch/session-1");
    let resume = ProviderSessionResumeInputs {
        requested_mode: Some("auto".to_owned()),
        work_roots: Some(vec![folder(&named)]),
        scratch_folder: Some(folder(&scratch)),
    };

    // Act
    let setup = restore_setup(&configured(root.path()), root.path(), &resume);

    // Assert
    assert_eq!(setup.requested_mode.as_deref(), Some("auto"));
    assert_eq!(setup.scratch_folder.as_deref(), Some(scratch.as_path()));
    assert!(setup.work_roots.router_auto);
    assert_eq!(
        setup.work_roots.roots,
        vec![
            root.path().join("configured"),
            root.path().join("tmp"),
            named.clone()
        ]
    );
    let command = setup
        .work_roots
        .ask_outside_command
        .expect("Claude's hook command");
    for inside in [&scratch, &named, &root.path().join("configured")] {
        assert!(
            command.contains(&inside.display().to_string()),
            "{command} names {}",
            inside.display()
        );
    }
}

/// Oracle: D1 R18, "a named mode keeps only its own folders": a record whose
/// create asked for a named mode rebuilds its scratch folder and no work
/// roots, and never Router auto, even if its creator named roots.
#[test]
fn a_named_mode_record_rebuilds_only_its_scratch_folder() {
    // Arrange
    let root = tempfile::tempdir().expect("root");
    let scratch = root.path().join("scratch/session-2");
    let resume = ProviderSessionResumeInputs {
        requested_mode: Some("default".to_owned()),
        work_roots: Some(vec![folder(&root.path().join("named"))]),
        scratch_folder: Some(folder(&scratch)),
    };

    // Act
    let setup = restore_setup(&configured(root.path()), root.path(), &resume);

    // Assert
    assert_eq!(setup.requested_mode.as_deref(), Some("default"));
    assert_eq!(setup.scratch_folder.as_deref(), Some(scratch.as_path()));
    assert_eq!(
        setup.work_roots,
        acp_client_runtime::SessionWorkRoots::default()
    );
}

/// Oracle: owner Decision 3, "old rows keep the fallback": a restore whose
/// record named no requested mode leaves the mode this Host already holds for
/// the session; one that named it replaces it.
#[test]
fn a_restore_without_a_recorded_mode_keeps_the_mode_already_held() {
    // Arrange
    let setup = BindingSessionSetup::default();
    let created = acp_client_runtime::PermissionModeReport {
        requested: Some("auto".to_owned()),
        actual: Some("auto".to_owned()),
        reason: None,
    };
    setup.record_created_mode("session-1", &created);

    // Act
    setup.record_restored_mode(
        "session-1",
        &acp_client_runtime::PermissionModeReport::default(),
    );
    let kept = setup.requested_mode("session-1");
    setup.record_restored_mode(
        "session-2",
        &acp_client_runtime::PermissionModeReport {
            requested: Some("plan".to_owned()),
            ..acp_client_runtime::PermissionModeReport::default()
        },
    );
    let restored = setup.requested_mode("session-2");

    // Assert
    assert_eq!(kept.requested_mode.as_deref(), Some("auto"));
    assert_eq!(restored.requested_mode.as_deref(), Some("plan"));
}
