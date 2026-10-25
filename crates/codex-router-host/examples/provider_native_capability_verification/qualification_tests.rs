//! Target qualification and ownership controls: the private canonical Router root, the
//! normal-build record, the Host's global Claude binding, the global Codex config
//! observation, and real-UID ownership against live setuid and foreign-owner processes.
//!
//! The live cases start only `/usr/bin/top` (setuid root on macOS) as this test's own
//! child and stop it; the foreign-owner case only reads PID 1's actual row.
use crate::global_claude_binding::resolve_global_claude;
use crate::journey_run::CodexConfigObservation;
use crate::native_exports::codex_config_digest;
use crate::oracle_test_support::expect_error;
use crate::process_identity::{
    observe_process_table, own_real_uid, owned_closure, parse_process_arguments,
    parse_process_table,
};
use crate::target_qualification::{
    adapter_install_root, process_running_from, require_endpoint_socket, require_native_binding,
    require_normal_build, require_private_router_root,
};
use crate::verification_models::{BuildRecord, NativeCapabilitiesPlan, QualifiedBinary};
use serde_json::json;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn canonical_temp() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let root = fs::canonicalize(directory.path()).unwrap();
    (directory, root)
}

fn private_directory(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn router_root_must_be_private_canonical_and_not_production() {
    let (_directory, root) = canonical_temp();
    let owner = own_real_uid().unwrap();
    let home = root.join("home");
    let router_root = root.join("router");
    private_directory(&router_root);
    require_private_router_root(&router_root, &home, owner).unwrap();
    let alias = root.join("alias");
    symlink(&router_root, &alias).unwrap();
    expect_error(
        require_private_router_root(&alias, &home, owner),
        "canonical path",
    );
    let production = home.join(".codex-router");
    private_directory(&production);
    expect_error(
        require_private_router_root(&production, &home, owner),
        "production",
    );
    expect_error(
        require_private_router_root(&router_root, &home, owner + 1),
        "not owned",
    );
    fs::set_permissions(&router_root, fs::Permissions::from_mode(0o755)).unwrap();
    expect_error(
        require_private_router_root(&router_root, &home, owner),
        "other users",
    );
}

fn binary(path: &str, sha256: &str) -> QualifiedBinary {
    QualifiedBinary {
        path: PathBuf::from(path),
        sha256: sha256.to_owned(),
    }
}

fn record(entries: serde_json::Value) -> BuildRecord {
    serde_json::from_value(json!({ "binaries": entries })).unwrap()
}

#[test]
fn only_the_recorded_normal_build_qualifies() {
    let normal = json!({"target": "codex-router", "path": "/build/codex-router", "sha256": "aa", "testGuardFeatures": [], "profile": "dev"});
    let candidate = binary("/build/codex-router", "aa");
    require_normal_build(&record(json!([normal])), &candidate, "codex-router").unwrap();
    let guarded = json!({"target": "codex-router", "path": "/build/codex-router", "sha256": "aa", "testGuardFeatures": ["keychain-test-support"]});
    expect_error(
        require_normal_build(&record(json!([guarded])), &candidate, "codex-router"),
        "test-guard",
    );
    expect_error(
        require_normal_build(
            &record(json!([normal])),
            &binary("/build/codex-router", "bb"),
            "codex-router",
        ),
        "differs from the build record",
    );
    expect_error(
        require_normal_build(&record(json!([])), &candidate, "codex-router"),
        "no single entry",
    );
    expect_error(
        require_normal_build(&record(json!([normal, normal])), &candidate, "codex-router"),
        "no single entry",
    );
}

fn executable_in(directory: &Path) -> PathBuf {
    fs::create_dir_all(directory).unwrap();
    let executable = directory.join("claude");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    executable
}

#[test]
fn global_claude_selection_mirrors_the_hosts_resolver() {
    let (_directory, root) = canonical_temp();
    let project = root.join("project/node_modules/.bin");
    let npx = root.join(".npm/_npx/fixture/node_modules/.bin");
    let dlx = root.join("pnpm/dlx/fixture/bin");
    for directory in [&project, &npx, &dlx] {
        executable_in(directory);
        assert_eq!(
            resolve_global_claude(directory.as_os_str()),
            None,
            "{}",
            directory.display()
        );
    }
    let nonexecutable = executable_in(&root.join("nonexecutable"));
    fs::set_permissions(&nonexecutable, fs::Permissions::from_mode(0o600)).unwrap();
    let sdk = executable_in(&root.join("node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64"));
    let sdk_alias = root.join("sdk-bin");
    fs::create_dir(&sdk_alias).unwrap();
    symlink(&sdk, sdk_alias.join("claude")).unwrap();
    let broken = root.join("broken");
    fs::create_dir(&broken).unwrap();
    symlink(root.join("missing"), broken.join("claude")).unwrap();
    for skipped in [
        &nonexecutable.parent().unwrap().to_owned(),
        &sdk_alias,
        &broken,
    ] {
        assert_eq!(
            resolve_global_claude(skipped.as_os_str()),
            None,
            "{}",
            skipped.display()
        );
    }
    assert_eq!(resolve_global_claude(OsStr::new("relative/bin")), None);
    // A normal global Claude Code package symlink stays eligible, after skipped entries.
    let package =
        executable_in(&root.join("global/lib/node_modules/@anthropic-ai/claude-code/bin"));
    let global = root.join("global/bin");
    fs::create_dir(&global).unwrap();
    symlink(&package, global.join("claude")).unwrap();
    let search = std::env::join_paths([&project, &npx, &dlx, &sdk_alias, &global]).unwrap();
    assert_eq!(
        resolve_global_claude(&search),
        Some(fs::canonicalize(&package).unwrap())
    );
}

fn claude_plan(native: &Path, search_path: Option<String>) -> NativeCapabilitiesPlan {
    let any = json!({"path": "/candidate/bin", "sha256": "00"});
    let process =
        json!({"pid": 1, "realUid": 501, "group": 1, "birth": "b", "executable": "/candidate/bin"});
    serde_json::from_value(json!({
        "repository": "/repo", "sourceHead": "0", "routerRoot": "/router", "routerPort": 1,
        "endpointSocket": "/router/agent-communication/router-sessions/claude-local.sock",
        "binaries": {"codexRouter": any, "agentCollaboration": any, "agentSessions": any},
        "binaryQualification": "/build/record.json", "expectedVersion": "0", "routerProcess": process,
        "hostProcess": process, "codexCli": any, "nativeExecutable": {"path": native, "sha256": "00"},
        "hostSearchPath": search_path, "endpointId": "claude-local", "sessionId": "s", "fixture": "/f.json",
        "nativeHomeSkillRoots": [], "nativeWorkspaceSkillDirs": [],
    }))
    .unwrap()
}

#[test]
fn declared_claude_must_be_the_hosts_global_selection() {
    let (_directory, root) = canonical_temp();
    let global = executable_in(&root.join("global"));
    let other = executable_in(&root.join("other"));
    let search = Some(root.join("global").display().to_string());
    require_native_binding(&claude_plan(&global, search.clone())).unwrap();
    expect_error(
        require_native_binding(&claude_plan(&other, search)),
        "differs from the Host's global PATH",
    );
    expect_error(
        require_native_binding(&claude_plan(&global, None)),
        "launch PATH",
    );
}

#[test]
fn codex_config_is_absent_digested_or_unobservable_never_confused() {
    let (_directory, home) = canonical_temp();
    assert_eq!(codex_config_digest(&home).unwrap(), "absent");
    let config = home.join(".codex/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "model = \"a\"\n").unwrap();
    let before = codex_config_digest(&home);
    assert_eq!(before.as_ref().unwrap().len(), 64);
    assert_eq!(
        CodexConfigObservation::compare(&before, &codex_config_digest(&home)),
        CodexConfigObservation::ProvenUnchanged
    );
    fs::write(&config, "model = \"b\"\n").unwrap();
    assert_eq!(
        CodexConfigObservation::compare(&before, &codex_config_digest(&home)),
        CodexConfigObservation::ChangedUnattributed
    );
    // A file that exists but cannot be read is unobservable, not absent, and leaves the
    // unchanged-preferences claim unknown rather than passed.
    fs::set_permissions(&config, fs::Permissions::from_mode(0o000)).unwrap();
    let denied = codex_config_digest(&home);
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        CodexConfigObservation::compare(&before, &denied),
        CodexConfigObservation::Unobservable
    );
    expect_error(denied, "unobservable");
}

/// Wait until ps lists `pid`, so the observation is of the live child.
fn table_with(pid: u32) -> BTreeMap<u32, crate::process_identity::ProcessIdentity> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let table = observe_process_table().unwrap();
        if table.contains_key(&pid) || Instant::now() > deadline {
            return table;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn live_setuid_child_is_owned_by_its_real_uid() {
    let owner = own_real_uid().unwrap();
    let mut child = Command::new("/usr/bin/top")
        .args(["-l", "0", "-s", "1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let table = table_with(pid);
    let effective = Command::new("/bin/ps")
        .args(["-o", "uid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let owned = owned_closure(std::process::id(), owner, &BTreeMap::new(), &table);
    child.kill().unwrap();
    child.wait().unwrap();
    // The control is meaningful only if the effective UID really differs.
    assert_eq!(String::from_utf8_lossy(&effective.stdout).trim(), "0");
    assert_eq!(table[&pid].real_uid, owner);
    assert!(owned.unwrap().contains_key(&pid));
}

#[test]
fn live_foreign_real_owner_is_rejected() {
    let owner = own_real_uid().unwrap();
    let mut table = observe_process_table().unwrap();
    let launchd = table.get_mut(&1).expect("PID 1 is listed");
    assert_ne!(launchd.real_uid, owner);
    // Present PID 1's actual row as if it descended from this test.
    launchd.parent = std::process::id();
    expect_error(
        owned_closure(std::process::id(), owner, &BTreeMap::new(), &table),
        "foreign real owner",
    );
}

#[test]
fn acp_adapter_binding_is_the_install_the_host_actually_runs() {
    let npx = Path::new(
        "/home/u/.npm/_npx/698eb38f/node_modules/@agentclientprotocol/claude-agent-acp/dist/acp-agent.js",
    );
    let npx_root = adapter_install_root(npx).unwrap();
    assert_eq!(npx_root, Path::new("/home/u/.npm/_npx/698eb38f"));
    let pnpm = Path::new(
        "/home/u/pnpm/global/5/.pnpm/@agentclientprotocol+claude-agent-acp@0.81.2/node_modules/@agentclientprotocol/claude-agent-acp/dist/acp-agent.js",
    );
    assert_eq!(
        adapter_install_root(pnpm).unwrap(),
        Path::new("/home/u/pnpm/global/5/.pnpm/@agentclientprotocol+claude-agent-acp@0.81.2")
    );
    assert_eq!(adapter_install_root(Path::new("/opt/acp-agent.js")), None);
    let row = |pid: u32, parent: u32| {
        format!("{pid} {parent} 501 {pid} Sat Oct 24 10:00:00 2026 /usr/local/bin/node")
    };
    let table = parse_process_table(&[row(10, 1), row(11, 10), row(12, 11), row(20, 1)].join("\n"))
        .unwrap();
    let host_closure = owned_closure(10, 501, &BTreeMap::new(), &table).unwrap();
    let arguments = parse_process_arguments(
        "   10 /candidate/codex-router host\n   11 npm exec @agentclientprotocol/claude-agent-acp@0.81.2\n   12 node /home/u/.npm/_npx/698eb38f/node_modules/.bin/claude-agent-acp\n   20 node /home/u/.npm/_npx/698eb38f/node_modules/.bin/claude-agent-acp\n",
    )
    .unwrap();
    assert_eq!(
        process_running_from(&npx_root, &host_closure, &arguments),
        Some(12)
    );
    // A same-version copy elsewhere is not what the Host runs.
    let pnpm_root = adapter_install_root(pnpm).unwrap();
    assert_eq!(
        process_running_from(&pnpm_root, &host_closure, &arguments),
        None
    );
    // A matching process outside the Host's descendants (PID 20) never binds it.
    let only_outside = parse_process_arguments(
        "20 node /home/u/.npm/_npx/698eb38f/node_modules/.bin/claude-agent-acp\n",
    )
    .unwrap();
    assert_eq!(
        process_running_from(&npx_root, &host_closure, &only_outside),
        None
    );
    // A different cache entry sharing a name prefix does not match.
    let prefix = parse_process_arguments(
        "12 node /home/u/.npm/_npx/698eb38f0/node_modules/.bin/claude-agent-acp\n",
    )
    .unwrap();
    assert_eq!(
        process_running_from(&npx_root, &host_closure, &prefix),
        None
    );
    assert!(parse_process_arguments("node without pid\n").is_err());
}

/// A private Router root under `/tmp` with its sessions directory. Sockets are bound
/// through the short `/tmp` alias; the canonical root is `/private/tmp/...` on macOS.
struct SocketSetup {
    _directory: TempDir,
    alias_root: PathBuf,
    canonical_root: PathBuf,
}

impl SocketSetup {
    fn new() -> Self {
        let directory = TempDir::new_in("/tmp").unwrap();
        let alias_root = directory.path().join("r");
        private_directory(&alias_root.join("agent-communication/router-sessions"));
        let canonical_root = fs::canonicalize(&alias_root).unwrap();
        Self {
            _directory: directory,
            alias_root,
            canonical_root,
        }
    }

    fn sessions(&self) -> PathBuf {
        self.alias_root.join("agent-communication/router-sessions")
    }

    /// A real listening socket at `path`, private unless `mode` says otherwise.
    fn bind(&self, path: &Path, mode: u32) -> UnixListener {
        let listener = UnixListener::bind(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        listener
    }

    fn plan(&self, socket: &Path) -> NativeCapabilitiesPlan {
        let mut plan = claude_plan(Path::new("/candidate/claude"), None);
        plan.router_root = self.canonical_root.clone();
        plan.endpoint_socket = socket.to_owned();
        plan
    }
}

#[test]
fn endpoint_socket_is_the_hosts_private_socket_under_the_canonical_root() {
    let owner = own_real_uid().unwrap();
    let setup = SocketSetup::new();
    let socket = setup.sessions().join("claude-local.sock");
    let _listener = setup.bind(&socket, 0o600);
    // The short alias spelling and the canonical spelling name the same socket.
    require_endpoint_socket(&setup.plan(&socket), owner).unwrap();
    let canonical = setup
        .canonical_root
        .join("agent-communication/router-sessions/claude-local.sock");
    require_endpoint_socket(&setup.plan(&canonical), owner).unwrap();
    expect_error(
        require_endpoint_socket(&setup.plan(&socket), owner + 1),
        "not owned",
    );
    expect_error(
        require_endpoint_socket(
            &setup.plan(Path::new(
                "agent-communication/router-sessions/claude-local.sock",
            )),
            owner,
        ),
        "absolute",
    );
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o666)).unwrap();
    expect_error(
        require_endpoint_socket(&setup.plan(&socket), owner),
        "accessible to other users",
    );
}

#[test]
fn another_directory_endpoint_or_file_kind_never_qualifies() {
    let owner = own_real_uid().unwrap();
    let setup = SocketSetup::new();
    // The right name in another directory.
    let elsewhere = setup.alias_root.join("other");
    private_directory(&elsewhere);
    let stray = elsewhere.join("claude-local.sock");
    let _stray = setup.bind(&stray, 0o600);
    expect_error(
        require_endpoint_socket(&setup.plan(&stray), owner),
        "outside the Router root's sessions directory",
    );
    // Another endpoint's socket in the right directory.
    let cursor = setup.sessions().join("cursor-local.sock");
    let _cursor = setup.bind(&cursor, 0o600);
    expect_error(
        require_endpoint_socket(&setup.plan(&cursor), owner),
        "not named claude-local.sock",
    );
    // A symlink named like the socket, pointing at a real socket elsewhere.
    let link = setup.sessions().join("claude-local.sock");
    symlink(&stray, &link).unwrap();
    expect_error(
        require_endpoint_socket(&setup.plan(&link), owner),
        "not a Unix socket",
    );
    // A regular file named like the socket.
    let file_setup = SocketSetup::new();
    let file = file_setup.sessions().join("claude-local.sock");
    fs::write(&file, "").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    expect_error(
        require_endpoint_socket(&file_setup.plan(&file), owner),
        "not a Unix socket",
    );
    expect_error(
        require_endpoint_socket(
            &file_setup.plan(&file_setup.sessions().join("missing/claude-local.sock")),
            owner,
        ),
        "unreadable",
    );
}
