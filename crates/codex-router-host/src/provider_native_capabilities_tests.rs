//! Launch workspace qualification must not adopt a requested session cwd.
use super::*;

fn launch(arguments: &[&str]) -> ExternalProviderLaunch {
    ExternalProviderLaunch {
        executable: PathBuf::from("/fixture/cursor-agent"),
        arguments: arguments.iter().map(|value| (*value).to_owned()).collect(),
        environment: vec![("HOME".into(), "/fixture/provider-home".into())],
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
    }
}

#[test]
fn native_capabilities_cursor_context_uses_actual_launch_workspace_and_environment() {
    let context = ProviderNativeReadContext::capture(
        ProviderKind::Cursor,
        &launch(&[
            "--disable-auto-update",
            "--disable-project-configs",
            "--workspace",
            "/fixture/workspace",
            "acp",
        ]),
    );
    assert_eq!(
        context.default_workspace(),
        Ok(PathBuf::from("/fixture/workspace"))
    );
    assert_eq!(context.home, Some(PathBuf::from("/fixture/provider-home")));
    let expected_launch_cwd = std::env::current_dir().expect("actual process launch cwd");
    let inherited = ProviderNativeReadContext::capture(ProviderKind::Cursor, &launch(&["acp"]));
    assert_eq!(inherited.default_workspace(), Ok(expected_launch_cwd));
}

#[test]
fn native_capabilities_cursor_unqualified_workspace_forms_fail_without_guessing() {
    for arguments in [
        vec!["--workspace", "saved-name", "acp"],
        vec!["--add-dir", "/fixture/additional", "acp"],
        vec!["--worktree", "acp"],
        vec!["--workspace"],
        vec!["--workspace=relative", "acp"],
    ] {
        let context = ProviderNativeReadContext::capture(ProviderKind::Cursor, &launch(&arguments));
        assert_eq!(
            context.default_workspace(),
            Err(CatalogueReadFailure::InvalidScope)
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_capabilities_export_bound_preserves_failure_and_rejects_oversized_output() {
    let mut command = tokio::process::Command::new("/bin/sh");
    command.args(["-c", "printf native-export"]);
    assert_eq!(
        bounded_native_output(&mut command, 32, &CancellationToken::new()).await,
        Ok(b"native-export".to_vec())
    );
    let mut command = tokio::process::Command::new("/bin/sh");
    command.args(["-c", "printf 12345"]);
    assert_eq!(
        bounded_native_output(&mut command, 4, &CancellationToken::new()).await,
        Err(CatalogueReadFailure::InvalidNativeData)
    );
    let mut command = tokio::process::Command::new("/bin/sh");
    command.args(["-c", "printf partial; exit 7"]);
    assert_eq!(
        bounded_native_output(&mut command, 32, &CancellationToken::new()).await,
        Err(CatalogueReadFailure::Unavailable)
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let mut command = tokio::process::Command::new("/does-not-exist/never-started");
    assert_eq!(
        bounded_native_output(&mut command, 32, &cancelled).await,
        Err(CatalogueReadFailure::Cancelled)
    );
}

#[test]
fn native_capabilities_cursor_lowering_requires_a_whole_native_reference_identifier() {
    let directory = tempfile::tempdir_in("/tmp").expect("fixture");
    let home = directory.path().join("home");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    for name in ["chosen", "prefix:tail", "prefix.tail"] {
        let path = workspace.join(".cursor/skills").join(name).join("SKILL.md");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("skill directory");
        std::fs::write(&path, "Native skill").expect("source");
    }
    let context = CursorFilesystemContext::new(home, workspace.clone()).expect("context");
    let native = read_cursor_filesystem_skills(
        &context,
        "2026.10.22-27dcf5f",
        &workspace,
        &CancellationToken::new(),
    )
    .expect("native files");
    let CatalogueSection::Supported { items, .. } =
        project_cursor_catalogue(native).expect("Host projection")
    else {
        panic!("supported")
    };
    assert_eq!(items.len(), 3);
    for skill in items {
        if skill.name.as_str() == "chosen" {
            assert!(
                matches!(skill.lowering, NativeSkillLowering::Qualified { canonical_command } if canonical_command.as_str() == "chosen")
            );
        } else {
            assert!(
                matches!(skill.lowering, NativeSkillLowering::Unqualified { .. }),
                "prefix matching must not select a different native skill"
            );
        }
    }
}

#[test]
fn native_capabilities_cursor_real_files_keep_located_failures_in_host_section() {
    // Literal source-role oracle: a malformed SKILL.md is a SkillSource;
    // root/path guesses cannot satisfy the source-accounting contract.
    let directory = tempfile::tempdir_in("/tmp").expect("fixture");
    let home = directory.path().join("home");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    let healthy = workspace.join(".cursor/skills/healthy/SKILL.md");
    let broken = workspace.join(".cursor/skills/broken/SKILL.md");
    for path in [&healthy, &broken] {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("skill directory");
    }
    std::fs::write(&healthy, "Healthy source").expect("healthy");
    std::fs::write(&broken, "---\nname: [broken\n---\n").expect("broken");
    let context = CursorFilesystemContext::new(home, workspace.clone()).expect("context");
    let source = read_cursor_filesystem_skills(
        &context,
        "2026.10.22-27dcf5f",
        &workspace,
        &CancellationToken::new(),
    )
    .expect("accounted roots");
    let section = project_cursor_catalogue(source).expect("Host projection");
    let CatalogueSection::Supported {
        items,
        enumeration,
        errors,
    } = section
    else {
        panic!("reader must preserve supported section");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name.as_str(), "healthy");
    assert_eq!(enumeration, SourceEnumeration::Accounted);
    assert_eq!(
        errors,
        vec![CatalogueSourceError {
            source: NativeErrorSource::SkillSource {
                actual_path: CapabilityPath::new(
                    std::fs::canonicalize(broken).expect("actual candidate")
                )
                .expect("path")
            },
            reason: CatalogueReadFailure::InvalidNativeData,
        }]
    );
}
