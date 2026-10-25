//! Refuse to observe anything but the declared, committed, signed, running candidate
//! the Lead started through the ordinary `codex-router serve`/`host` lifetimes.
use crate::bounded_process::run_bounded;
use crate::global_claude_binding::resolve_global_claude;
use crate::native_fixture::{fixture_relative_files, fixture_worktree_state, write_private};
use crate::process_identity::{
    ProcessIdentity, observe_process_arguments, observe_process_table, own_real_uid, owned_closure,
    recorded_process_live,
};
use crate::verification_models::{
    BuildRecord, FixtureRecord, NativeCapabilitiesPlan, ProviderEndpoint, QualifiedBinary,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    process::Command,
};

const NATIVE_VERSION_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);
const NATIVE_VERSION_MAX_BYTES: usize = 64 * 1024;

/// Release identity team; debug builds sign as `dev.shravansunder.<binary>.debug`.
const SIGNING_TEAM: &str = "974QD84WVC";

pub(crate) struct QualifiedTarget {
    pub(crate) plan: NativeCapabilitiesPlan,
    pub(crate) fixture: FixtureRecord,
    pub(crate) service_id: String,
}

pub(crate) fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn git_output(repository: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {} failed", arguments.join(" ")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn require_hash(binary: &QualifiedBinary, label: &str) -> Result<(), String> {
    if file_sha256(&binary.path)? != binary.sha256 {
        return Err(format!("{label} hash mismatch"));
    }
    Ok(())
}

/// Strict signature with the debug identifier and team, as the signing runner applies.
fn require_debug_signature(binary: &QualifiedBinary, name: &str) -> Result<(), String> {
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(&binary.path)
        .status()
        .map_err(|error| format!("codesign could not run: {error}"))?;
    if !verified.success() {
        return Err(format!("{name} fails strict signature verification"));
    }
    let details = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(&binary.path)
        .output()
        .map_err(|error| format!("codesign could not run: {error}"))?;
    let signature = String::from_utf8_lossy(&details.stderr);
    if !signature.contains(&format!("Identifier=dev.shravansunder.{name}.debug"))
        || !signature.contains(&format!("TeamIdentifier={SIGNING_TEAM}"))
    {
        return Err(format!("{name} debug signing identity mismatch"));
    }
    Ok(())
}

/// The run's own Router root: canonical as declared (no alias into another root), never
/// the production root, owned by this verifier's real user and private to it.
pub(crate) fn require_private_router_root(
    root: &Path,
    home: &Path,
    owner: u32,
) -> Result<(), String> {
    let canonical =
        fs::canonicalize(root).map_err(|error| format!("Router root unreadable: {error}"))?;
    if canonical != root {
        return Err("Router root is not declared by its canonical path".to_owned());
    }
    let production =
        fs::canonicalize(home.join(".codex-router")).unwrap_or_else(|_| home.join(".codex-router"));
    if canonical.starts_with(&production) || canonical.starts_with(home.join(".codex-router")) {
        return Err("production Router root is outside this journey".to_owned());
    }
    let metadata =
        fs::metadata(&canonical).map_err(|error| format!("Router root unreadable: {error}"))?;
    if metadata.uid() != owner {
        return Err("Router root is not owned by this verifier's real user".to_owned());
    }
    if metadata.mode() & 0o077 != 0 {
        return Err("Router root is readable or writable by other users".to_owned());
    }
    Ok(())
}

/// The declared endpoint socket is the Host's actual socket for this endpoint: an
/// absolute path whose final component is itself a Unix socket (not a regular file or
/// a symlink to one) named `<endpointId>.sock`, in a directory that canonically is
/// `<routerRoot>/agent-communication/router-sessions`, owned by this verifier's real
/// user and private to it. A short alias spelling of that directory qualifies; another
/// directory, endpoint or owner does not.
pub(crate) fn require_endpoint_socket(
    plan: &NativeCapabilitiesPlan,
    owner: u32,
) -> Result<(), String> {
    let socket = &plan.endpoint_socket;
    if !socket.is_absolute() {
        return Err("endpoint socket is not an absolute path".to_owned());
    }
    if socket.file_name().and_then(OsStr::to_str) != Some(plan.endpoint_socket_name().as_str()) {
        return Err(format!(
            "endpoint socket is not named {}",
            plan.endpoint_socket_name()
        ));
    }
    let metadata = fs::symlink_metadata(socket)
        .map_err(|error| format!("endpoint socket unreadable: {error}"))?;
    if !metadata.file_type().is_socket() {
        return Err("endpoint socket path is not a Unix socket".to_owned());
    }
    let parent = socket.parent().ok_or("endpoint socket has no directory")?;
    let actual = fs::canonicalize(parent)
        .map_err(|error| format!("endpoint socket directory unreadable: {error}"))?;
    let expected = fs::canonicalize(plan.router_sessions_directory())
        .map_err(|error| format!("Router sessions directory unreadable: {error}"))?;
    if actual != expected {
        return Err("endpoint socket is outside the Router root's sessions directory".to_owned());
    }
    if metadata.uid() != owner {
        return Err("endpoint socket is not owned by this verifier's real user".to_owned());
    }
    if metadata.mode() & 0o077 != 0 {
        return Err("endpoint socket is accessible to other users".to_owned());
    }
    Ok(())
}

/// A normal build: the build's own record lists this exact binary with no test-guard
/// features (a `keychain-test-support` build swaps in a fixture key).
pub(crate) fn require_normal_build(
    record: &BuildRecord,
    binary: &QualifiedBinary,
    name: &str,
) -> Result<(), String> {
    let mut entries = record.binaries.iter().filter(|entry| entry.target == name);
    let (Some(entry), None) = (entries.next(), entries.next()) else {
        return Err(format!("build record has no single entry for {name}"));
    };
    if entry.path != binary.path || entry.sha256 != binary.sha256 {
        return Err(format!("{name} differs from the build record"));
    }
    if !entry.test_guard_features.is_empty() {
        return Err(format!("{name} is a test-guard build, not a normal build"));
    }
    Ok(())
}

/// The native executable is the one the Host actually binds: for `claude-local`, the
/// global Claude its launch PATH selects; for Cursor, the declared hashed executable.
pub(crate) fn require_native_binding(
    plan: &NativeCapabilitiesPlan,
) -> Result<serde_json::Value, String> {
    let declared = fs::canonicalize(&plan.native_executable.path)
        .map_err(|error| format!("native executable unreadable: {error}"))?;
    if plan.endpoint_id != ProviderEndpoint::ClaudeLocal {
        return Ok(json!({"canonicalPath": declared}));
    }
    let search_path = plan
        .host_search_path
        .as_deref()
        .ok_or("claude-local needs the Host's launch PATH to qualify its binding")?;
    let selected = resolve_global_claude(OsStr::new(search_path))
        .ok_or("the Host's launch PATH selects no qualified global Claude executable")?;
    if selected != declared {
        return Err(
            "declared Claude executable differs from the Host's global PATH selection".to_owned(),
        );
    }
    Ok(json!({"canonicalPath": selected, "selectedBy": "hostSearchPath"}))
}

/// The package install root an adapter entry file belongs to: the parent of its
/// innermost `node_modules` (for npx, the `_npx/<hash>` cache entry).
pub(crate) fn adapter_install_root(entry: &Path) -> Option<PathBuf> {
    entry
        .ancestors()
        .find(|ancestor| {
            ancestor
                .file_name()
                .is_some_and(|name| name == "node_modules")
        })
        .and_then(Path::parent)
        .map(Path::to_owned)
}

/// The live Host descendant whose argument line runs from `install_root`, if any.
pub(crate) fn process_running_from(
    install_root: &Path,
    host_closure: &BTreeMap<u32, ProcessIdentity>,
    arguments: &BTreeMap<u32, String>,
) -> Option<u32> {
    let root = format!("{}/", install_root.display());
    host_closure
        .keys()
        .copied()
        .find(|pid| arguments.get(pid).is_some_and(|line| line.contains(&root)))
}

/// For `claude-local`, the declared ACP adapter is the one the live Host actually runs:
/// its hash matches and a Host descendant runs from its install root. A same-version
/// copy elsewhere (a global pnpm install) does not qualify. Only the PID and root are
/// recorded, never argument lines.
pub(crate) fn require_adapter_binding(
    plan: &NativeCapabilitiesPlan,
    owner: u32,
    table: &BTreeMap<u32, ProcessIdentity>,
) -> Result<serde_json::Value, String> {
    if plan.endpoint_id != ProviderEndpoint::ClaudeLocal {
        return Ok(serde_json::Value::Null);
    }
    let adapter = plan
        .acp_adapter
        .as_ref()
        .ok_or("claude-local needs the ACP adapter the Host runs")?;
    require_hash(adapter, "ACP adapter")?;
    let install_root = adapter_install_root(&adapter.path)
        .ok_or("ACP adapter entry is not inside a node_modules package")?;
    let host_closure = owned_closure(plan.host_process.pid, owner, &BTreeMap::new(), table)?;
    let pid = process_running_from(&install_root, &host_closure, &observe_process_arguments()?)
        .ok_or("no live Host descendant runs the declared ACP adapter; binding unobserved")?;
    Ok(json!({
        "entry": adapter.path,
        "sha256": adapter.sha256,
        "installRoot": install_root,
        "hostDescendantPid": pid,
    }))
}

/// The fixture worktree is still the genuine linked worktree at the declared candidate
/// commit, holding the declared fixture files with their recorded canaries. Other
/// changes (a native provider writing into its workspace) are returned and recorded,
/// never hidden; materialization itself refused anything but a clean worktree.
fn require_fixture_worktree(
    plan: &NativeCapabilitiesPlan,
    fixture: &FixtureRecord,
) -> Result<Vec<PathBuf>, String> {
    let (head, changes) = fixture_worktree_state(&fixture.workspace)?;
    if head != plan.source_head || fixture.workspace_head != plan.source_head {
        return Err("fixture worktree is not at the declared candidate commit".to_owned());
    }
    let declared = fixture_relative_files(fixture);
    if declared.len() != 3 || !declared.iter().all(|file| changes.contains(file)) {
        return Err("declared fixture files are missing from the fixture worktree".to_owned());
    }
    for (file, canary) in [
        (&fixture.skill_file, &fixture.canary),
        (&fixture.second_skill_file, &fixture.second_canary),
    ] {
        let body = fs::read_to_string(file)
            .map_err(|error| format!("fixture skill unreadable: {error}"))?;
        if !body.contains(canary.as_str()) {
            return Err("fixture skill no longer holds its recorded canary".to_owned());
        }
    }
    Ok(changes
        .into_iter()
        .filter(|change| !declared.contains(change))
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostPublication {
    service_id: String,
    service_version: String,
    router_proxy_endpoint: String,
}

/// Blocking checks (git, codesign, ps, files); run it in one `spawn_blocking` unit.
/// `router_version` is the health reading taken on the runtime just before.
pub(crate) fn qualify_target(
    plan: NativeCapabilitiesPlan,
    evidence: &Path,
    home: &Path,
    router_version: String,
) -> Result<QualifiedTarget, String> {
    let owner = own_real_uid()?;
    require_private_router_root(&plan.router_root, home, owner)?;
    require_endpoint_socket(&plan, owner)?;
    let head = git_output(&plan.repository, &["rev-parse", "HEAD"])?;
    if head != plan.source_head
        || !git_output(&plan.repository, &["status", "--porcelain"])?.is_empty()
    {
        return Err("candidate source is not the declared clean commit".to_owned());
    }
    let build_record: BuildRecord = serde_json::from_slice(
        &fs::read(&plan.binary_qualification)
            .map_err(|error| format!("build record unreadable: {error}"))?,
    )
    .map_err(|error| format!("build record does not decode: {error}"))?;
    for (binary, name) in [
        (&plan.binaries.codex_router, "codex-router"),
        (&plan.binaries.agent_collaboration, "agent-collaboration"),
        (&plan.binaries.agent_sessions, "agent-sessions"),
    ] {
        require_normal_build(&build_record, binary, name)?;
        require_hash(binary, name)?;
        require_debug_signature(binary, name)?;
    }
    require_hash(&plan.codex_cli, "installed Codex")?;
    require_hash(&plan.native_executable, "native provider executable")?;
    let native_binding = require_native_binding(&plan)?;
    let table = observe_process_table()?;
    for (recorded, label) in [
        (&plan.router_process, "Router"),
        (&plan.host_process, "Host"),
    ] {
        if recorded.real_uid != owner {
            return Err(format!("{label} is not owned by this verifier's real user"));
        }
        if !recorded_process_live(recorded, &table) {
            return Err(format!("recorded {label} process is not the live one"));
        }
        if recorded.executable != plan.binaries.codex_router.path {
            return Err(format!(
                "{label} does not run the qualified codex-router binary"
            ));
        }
    }
    let adapter_binding = require_adapter_binding(&plan, owner, &table)?;
    let version = router_version;
    if version != plan.expected_version {
        return Err("Router version mismatch".to_owned());
    }
    let publication: HostPublication = serde_json::from_slice(
        &fs::read(plan.service_directory().join("service.json"))
            .map_err(|error| format!("Host publication unreadable: {error}"))?,
    )
    .map_err(|error| format!("Host publication does not decode: {error}"))?;
    if publication.service_version != plan.expected_version
        || publication.router_proxy_endpoint != format!("127.0.0.1:{}", plan.router_port)
    {
        return Err("published Host version or Router endpoint mismatch".to_owned());
    }
    // Only now, with its hash and binding qualified, is the native executable run; the
    // caller holds this blocking unit in `block_in_place`, so the runtime is reachable.
    let mut version_command = tokio::process::Command::new(&plan.native_executable.path);
    version_command.arg("--version");
    let native_version = tokio::runtime::Handle::current().block_on(run_bounded(
        version_command,
        "native --version",
        NATIVE_VERSION_DEADLINE,
        NATIVE_VERSION_MAX_BYTES,
        NATIVE_VERSION_MAX_BYTES,
    ))?;
    if !native_version.status.success() {
        return Err("native --version failed".to_owned());
    }
    let fixture: FixtureRecord = serde_json::from_slice(
        &fs::read(&plan.fixture).map_err(|error| format!("fixture record unreadable: {error}"))?,
    )
    .map_err(|error| format!("fixture record does not decode: {error}"))?;
    let worktree_changes = require_fixture_worktree(&plan, &fixture)?;
    let record = json!({
        "sourceHead": head,
        "binaries": plan.binaries,
        "routerVersion": version,
        "endpointSocket": plan.endpoint_socket,
        "serviceId": publication.service_id,
        "routerProcess": plan.router_process,
        "hostProcess": plan.host_process,
        "verifierRealUid": owner,
        "codex": plan.codex_cli,
        "native": {
            "endpoint": plan.endpoint_id.as_str(),
            "executable": plan.native_executable,
            "version": String::from_utf8_lossy(&native_version.stdout).trim(),
            "binding": native_binding,
            "acpAdapter": adapter_binding,
            // The Host launches providers with the normal home; no config-dir override.
            "home": home,
            "claudeConfigDirectoryOverride": std::env::var_os("CLAUDE_CONFIG_DIR").is_some(),
        },
        "fixtureWorktree": {
            "head": fixture.workspace_head,
            "changesBeyondFixture": worktree_changes,
        },
    });
    write_private(
        &evidence.join("qualification.json"),
        &serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?,
    )?;
    Ok(QualifiedTarget {
        service_id: publication.service_id,
        plan,
        fixture,
    })
}
