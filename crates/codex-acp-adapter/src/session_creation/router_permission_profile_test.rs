use super::*;

const HOME: &str = "/Users/owner";
const SCRATCH: &str = "/owner/scratch/root";
const TEMPORARY_DIRECTORIES: [&str; 2] = ["/private/var/folders/xy/T", "/private/tmp"];

fn host(symlinked_locations: BTreeSet<&'static str>) -> HostLocations {
    HostLocations {
        home: PathBuf::from(HOME),
        temporary_directories: TEMPORARY_DIRECTORIES.map(PathBuf::from).to_vec(),
        symlinked_locations,
    }
}

fn profile(access: RouterAccess) -> RouterSessionProfile {
    RouterSessionProfile::with_host(
        access,
        Path::new("/repo"),
        Path::new(SCRATCH),
        &host(BTreeSet::new()),
    )
}

fn under_home(locations: &[&str]) -> Vec<String> {
    locations
        .iter()
        .map(|location| format!("{HOME}/{location}"))
        .collect()
}

fn native_response(profile_id: &str, roots: &[String], network_access: bool) -> Value {
    json!({
        "activePermissionProfile":{"id":profile_id},
        "sandbox":{
            "type":"workspaceWrite",
            "writableRoots":roots,
            "networkAccess":network_access,
            "excludeTmpdirEnvVar":true,
            "excludeSlashTmp":true
        }
    })
}

fn filesystem(config: &Map<String, Value>, name: &str) -> Map<String, Value> {
    config[&format!("permissions.{name}.filesystem")]
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn each_mode_grants_scratch_and_its_own_tool_locations() {
    // Arrange
    let workspace = profile(RouterAccess::WorkspaceWrite).native_config();
    let restricted = profile(RouterAccess::WriteRestricted).native_config();

    // Act
    let workspace_filesystem = filesystem(&workspace, "router-workspace-write");
    let restricted_filesystem = filesystem(&restricted, "router-write-restricted");

    // Assert: workspace-write writes toolchain homes; write-restricted only caches and stores.
    for root in under_home(&WORKSPACE_TOOL_LOCATIONS) {
        assert_eq!(workspace_filesystem[&root], "write", "{root}");
    }
    for root in under_home(&RESTRICTED_TOOL_LOCATIONS) {
        assert_eq!(restricted_filesystem[&root], "write", "{root}");
    }
    for toolchain in under_home(&[".rustup", ".cargo", "Library/pnpm"]) {
        assert!(
            !restricted_filesystem.contains_key(&toolchain),
            "{toolchain}"
        );
    }
    for (config, name, parent) in [
        (&workspace, "router-workspace-write", ":workspace"),
        (&restricted, "router-write-restricted", ":read-only"),
    ] {
        assert_eq!(config["default_permissions"], name);
        assert_eq!(config[&format!("permissions.{name}.extends")], parent);
        assert_eq!(filesystem(config, name)[SCRATCH], "write");
        // Assert: no proxy switch; both modes keep the root's direct network.
        assert!(!config.contains_key("features.network_proxy.enabled"));
    }
    // Assert: only write-restricted adds the repository's tmp and docs/wip and the
    // system temporary directories that `:workspace` already grants natively.
    for root in ["/repo/tmp", "/repo/docs/wip"]
        .into_iter()
        .chain(TEMPORARY_DIRECTORIES)
    {
        assert_eq!(restricted_filesystem[root], "write");
        assert!(!workspace_filesystem.contains_key(root));
    }
}

/// Oracle: R4 (only the session's scratch folder, not the system $TMPDIR or
/// /tmp) under workspace-write, and the Lead's TMPDIR decision (push 01a1da9b).
#[test]
fn workspace_write_withholds_system_temporary_directories_and_points_tmpdir_at_scratch() {
    // Arrange
    let workspace = profile(RouterAccess::WorkspaceWrite).native_config();
    let restricted = profile(RouterAccess::WriteRestricted).native_config();

    // Act
    let workspace_filesystem = filesystem(&workspace, "router-workspace-write");
    let restricted_filesystem = filesystem(&restricted, "router-write-restricted");

    // Assert: the `:workspace` parent's temporary-directory writes become reads.
    assert_eq!(workspace_filesystem[":tmpdir"], "read");
    assert_eq!(workspace_filesystem[":slash_tmp"], "read");
    assert_eq!(workspace["shell_environment_policy.set.TMPDIR"], SCRATCH);
    // Assert: write-restricted keeps its established explicit temporary grants.
    assert!(!restricted_filesystem.contains_key(":tmpdir"));
    assert!(!restricted_filesystem.contains_key(":slash_tmp"));
    assert!(!restricted.contains_key("shell_environment_policy.set.TMPDIR"));
}

#[test]
fn executables_and_startup_files_stay_read_only_and_are_not_expected_back() {
    // Arrange
    let workspace = profile(RouterAccess::WorkspaceWrite);

    // Act
    let filesystem = filesystem(&workspace.native_config(), "router-workspace-write");

    // Assert: PATH entries and sourced files are read-only inside writable parents.
    for path in [
        ".cargo/bin",
        ".cargo/env",
        "Library/pnpm/bin",
        ".gradle/init.d",
    ] {
        assert_eq!(filesystem[&format!("{HOME}/{path}")], "read", "{path}");
    }
    assert_eq!(filesystem[&format!("{HOME}/.cargo")], "write");
    assert!(
        !workspace
            .writable_roots
            .contains(&format!("{HOME}/.cargo/bin"))
    );
}

#[test]
fn symlinked_tool_locations_are_not_granted() {
    // Arrange: a cache moved to another volume through a symlink.
    let profile = RouterSessionProfile::with_host(
        RouterAccess::WorkspaceWrite,
        Path::new("/repo"),
        Path::new(SCRATCH),
        &host(BTreeSet::from([".cache"])),
    );

    // Act
    let filesystem = filesystem(&profile.native_config(), "router-workspace-write");

    // Assert: Codex refuses a whole sandbox with a symlinked writable root.
    assert!(!filesystem.contains_key(&format!("{HOME}/.cache")));
    assert_eq!(filesystem[&format!("{HOME}/Library/Caches")], "write");
}

#[tokio::test]
async fn symlinked_components_below_home_are_detected() {
    // Arrange
    let home = std::env::temp_dir().join(format!(
        "router-symlink-home-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let elsewhere = home.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, home.join(".cache")).unwrap();

    // Act & assert
    assert!(has_symlinked_component(&home, ".cache/uv").await);
    assert!(!has_symlinked_component(&home, "elsewhere").await);
    assert!(!has_symlinked_component(&home, ".missing/child").await);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn native_settings_report_each_difference_from_the_requested_profile() {
    // Arrange
    let workspace = profile(RouterAccess::WorkspaceWrite);
    let mut granted = under_home(&WORKSPACE_TOOL_LOCATIONS);
    granted.push(SCRATCH.to_owned());
    let mut with_unexpected_root = granted.clone();
    with_unexpected_root.push("/other/write-root".to_owned());
    let mut unsandboxed = native_response("router-workspace-write", &granted, true);
    unsandboxed["sandbox"]["type"] = json!("dangerFullAccess");
    // R4: native still granting the system temporary directories is a sandbox
    // difference, not silently accepted.
    let mut tmpdir_writable = native_response("router-workspace-write", &granted, true);
    tmpdir_writable["sandbox"]["excludeTmpdirEnvVar"] = json!(false);
    let mut slash_tmp_writable = native_response("router-workspace-write", &granted, true);
    slash_tmp_writable["sandbox"]["excludeSlashTmp"] = json!(false);

    // Act/assert: report exact field classes, with no native paths in the report.
    assert_eq!(
        workspace.observed_differences(&native_response("router-workspace-write", &granted, true)),
        vec![]
    );
    for (response, expected) in [
        (
            json!({}),
            vec![
                PermissionDifference::Profile,
                PermissionDifference::Sandbox,
                PermissionDifference::WritableRoots,
                PermissionDifference::Network,
            ],
        ),
        (
            native_response("router-workspace-write", &with_unexpected_root, true),
            vec![PermissionDifference::WritableRoots],
        ),
        (
            native_response("router-workspace-write", &granted, false),
            vec![PermissionDifference::Network],
        ),
        (unsandboxed, vec![PermissionDifference::Sandbox]),
        (tmpdir_writable, vec![PermissionDifference::Sandbox]),
        (slash_tmp_writable, vec![PermissionDifference::Sandbox]),
        (
            native_response("router-write-restricted", &granted, true),
            vec![PermissionDifference::Profile],
        ),
    ] {
        assert_eq!(workspace.observed_differences(&response), expected);
    }
    let mut malformed = native_response("router-workspace-write", &granted, true);
    malformed["sandbox"]["writableRoots"] = json!([123]);
    assert_eq!(
        workspace.observed_differences(&malformed),
        vec![PermissionDifference::WritableRoots]
    );
}

#[test]
fn write_restricted_expects_its_stores_work_areas_and_temporary_directories() {
    // Arrange
    let restricted = profile(RouterAccess::WriteRestricted);
    let mut granted = under_home(&RESTRICTED_TOOL_LOCATIONS);
    granted.extend(
        [SCRATCH, "/repo/tmp", "/repo/docs/wip"]
            .into_iter()
            .chain(TEMPORARY_DIRECTORIES)
            .map(str::to_owned),
    );
    let without_temporary = granted
        .iter()
        .filter(|root| !TEMPORARY_DIRECTORIES.contains(&root.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    // Act & assert
    assert!(
        restricted
            .observed_differences(&native_response("router-write-restricted", &granted, true))
            .is_empty()
    );
    assert_eq!(
        restricted.observed_differences(&native_response(
            "router-write-restricted",
            &without_temporary,
            true
        )),
        vec![PermissionDifference::WritableRoots]
    );
}

#[test]
fn canonical_scratch_reports_exact_permission_differences_and_keeps_read_exceptions() {
    // Arrange: the canonical owned scratch spelling is already selected.
    let restricted = RouterSessionProfile::with_host(
        RouterAccess::WriteRestricted,
        Path::new("/repo"),
        Path::new("/private/tmp/owned/scratch/session-root"),
        &host(BTreeSet::new()),
    );
    let mut granted = under_home(&RESTRICTED_TOOL_LOCATIONS);
    granted.extend(
        [
            "/private/tmp/owned/scratch/session-root",
            "/repo/tmp",
            "/repo/docs/wip",
        ]
        .into_iter()
        .chain(TEMPORARY_DIRECTORIES)
        .map(str::to_owned),
    );
    let accepted = native_response("router-write-restricted", &granted, true);
    let mut missing_scratch = granted.clone();
    missing_scratch.retain(|root| root != "/private/tmp/owned/scratch/session-root");
    let mut extra_root = granted.clone();
    extra_root.push("/other/write-root".to_owned());
    let mut missing_network = accepted.clone();
    missing_network["sandbox"]
        .as_object_mut()
        .unwrap()
        .remove("networkAccess");

    // Act & assert: alias handling must not turn exact policy into subset matching.
    assert!(restricted.observed_differences(&accepted).is_empty());
    for rejected in [
        native_response("router-write-restricted", &missing_scratch, true),
        native_response("router-write-restricted", &extra_root, true),
        native_response("router-workspace-write", &granted, true),
        native_response("router-write-restricted", &granted, false),
        missing_network,
    ] {
        assert!(!restricted.observed_differences(&rejected).is_empty());
    }
    let filesystem = filesystem(&restricted.native_config(), "router-write-restricted");
    assert_eq!(
        filesystem["/private/tmp/owned/scratch/session-root"],
        "write"
    );
    assert!(!filesystem.contains_key("/tmp/owned/scratch/session-root"));
    assert_eq!(
        filesystem[&format!("{HOME}/Library/Caches/pnpm/dlx")],
        "read"
    );
    assert_eq!(filesystem[&format!("{HOME}/.cargo/env")], "read");
}

#[tokio::test]
async fn absent_recorded_scratch_keeps_its_original_grant_spelling() {
    // Arrange: a recorded path whose owned directory no longer exists.
    let identity = String::from(collaboration_protocol::OperationId::generate());
    let scratch = PathBuf::from(format!(
        "/tmp/router-absent-scratch-{identity}/scratch/root"
    ));
    assert!(tokio::fs::symlink_metadata(&scratch).await.is_err());

    // Act & assert: do not invent a new canonical grant on a failed lookup.
    assert_eq!(scratch_root_for_native(&scratch).await, scratch);
}
