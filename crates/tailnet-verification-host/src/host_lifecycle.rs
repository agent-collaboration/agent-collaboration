use std::collections::BTreeMap;
use std::fs::{DirBuilder, OpenOptions};
use std::net::TcpListener;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::bounded_command::require_continuing;
use crate::driver_error::{DriverError, DriverResult};
use crate::driver_records::{read_record, save_record, sha256};
use crate::host_plan::{HostPlan, require_deadline, validate_fresh_root};
use crate::host_qualification::{
    contains_port, qualify_artifacts, require_http_serve_backend, require_serve_during,
    require_serve_restored, serve_snapshot,
};
use crate::mcp_observation::{
    NativeReadiness, observe_http_boundary, observe_local_inventory, observe_native_readiness,
    parse_manifest_mcp, tool,
};
use crate::owned_host::with_owned_child;
use crate::process_identity::{ProcessIdentity, lookup_identity, observe_owned, require_protected};
use crate::profile_projection::ProfileProjection;

pub fn host_readiness_until(allocation_remaining: chrono::TimeDelta, monotonic_now: f64) -> f64 {
    let seconds = allocation_remaining.num_microseconds().unwrap_or(i64::MAX) as f64 / 1_000_000.0;
    monotonic_now + seconds.min(150.0)
}

fn require_allocation(plan: &HostPlan) -> DriverResult<()> {
    require_continuing()?;
    require_deadline(&plan.deadline)
}
fn private_directory(path: &Path) -> DriverResult<()> {
    DirBuilder::new().mode(0o700).create(path)?;
    Ok(())
}

fn driver_binding(plan: &HostPlan) -> DriverResult<Value> {
    let sources = [
        "main.rs",
        "lib.rs",
        "driver_error.rs",
        "driver_records.rs",
        "bounded_command.rs",
        "host_plan.rs",
        "host_lifecycle.rs",
        "host_qualification.rs",
        "mcp_observation.rs",
        "process_identity.rs",
        "owned_host.rs",
        "profile_projection.rs",
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = BTreeMap::new();
    for name in sources {
        files.insert(name, sha256(&root.join(name))?);
    }
    Ok(
        json!({ "driverSHA256": sha256(&std::env::current_exe()?)?, "plan": plan, "driverFiles": files }),
    )
}

fn spawn_host(plan: &HostPlan, binary: &Path, native: &Path) -> DriverResult<Child> {
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(plan.evidence().join("host.stdout"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(plan.evidence().join("host.stderr"))?;
    require_allocation(plan)?;
    Ok(Command::new(std::env::current_exe()?)
        .arg("__launch-host")
        .arg(binary)
        .args(["host", "--router-root"])
        .arg(&plan.router_root)
        .arg("--port")
        .arg(plan.port.to_string())
        .args([
            "--mcp-bind",
            "127.0.0.1:0",
            "--require-debug-isolation",
            "--remote-access",
            "tailscale",
            "--tailscale-https-port",
        ])
        .arg(plan.https_port.to_string())
        .env(
            "CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET",
            native.join("app-server.sock"),
        )
        .stdout(stdout)
        .stderr(stderr)
        .spawn()?)
}

fn wait_for_host_image(child: &mut Child, binary: &Path) -> DriverResult<()> {
    let expected = binary.canonicalize()?;
    let until = Instant::now() + Duration::from_secs(5);
    let pid = i32::try_from(child.id()).map_err(|_| DriverError::Runtime("Host PID overflow"))?;
    loop {
        if child.try_wait()?.is_some() {
            return Err(DriverError::Runtime("Host exited before readiness"));
        }
        if let Some(identity) = lookup_identity(pid)?
            && Path::new(&identity.executable)
                .canonicalize()
                .is_ok_and(|path| path == expected)
        {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(DriverError::Invalid(
                "actual Host image differs from declared binary",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn await_readiness(
    plan: &HostPlan,
    child: &mut Child,
    known: &mut BTreeMap<i32, ProcessIdentity>,
    before: &Value,
) -> DriverResult<()> {
    let root = i32::try_from(child.id()).map_err(|_| DriverError::Runtime("Host PID overflow"))?;
    let remaining = plan.deadline.with_timezone(&chrono::Utc) - chrono::Utc::now();
    let until =
        Instant::now() + Duration::from_secs_f64(host_readiness_until(remaining, 0.0).max(0.0));
    let mut invocation: Option<(String, String)> = None;
    while Instant::now() < until {
        require_allocation(plan)?;
        if plan.evidence().join("stop.json").exists() {
            return Err(DriverError::Runtime("explicit stop before readiness"));
        }
        if child.try_wait()?.is_some() {
            return Err(DriverError::Runtime("Host exited before readiness"));
        }
        observe_owned(root, known)?;
        let manifest_path = plan.service().join("service.json");
        if manifest_path.exists() {
            let manifest: Value = read_record(&manifest_path)?;
            if manifest.get("serviceVersion") != Some(&json!(plan.expected_version))
                || manifest.get("routerProxyEndpoint")
                    != Some(&json!(format!("127.0.0.1:{}", plan.port)))
            {
                return Err(DriverError::Invalid("Host manifest binding mismatch"));
            }
            let service_id = manifest
                .get("serviceId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or(DriverError::Invalid("Host service identity absent"))?;
            let service_epoch = manifest
                .get("serviceEpoch")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or(DriverError::Invalid("Host service identity absent"))?;
            let current = (service_id.to_owned(), service_epoch.to_owned());
            if invocation
                .as_ref()
                .is_some_and(|previous| previous != &current)
            {
                return Err(DriverError::Invalid(
                    "Host invocation changed during readiness",
                ));
            }
            invocation = Some(current);
            let local = parse_manifest_mcp(&manifest)?;
            let inventory = tool(
                &plan.service().join("control.sock"),
                "endpoints_list",
                &json!({}),
            )?;
            if observe_native_readiness(&inventory, service_id, service_epoch)?
                == NativeReadiness::Starting
            {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let during = serve_snapshot(&plan.tailscale)?;
            require_serve_during(before, &during, plan.https_port)?;
            let session = require_http_serve_backend(&during, &local, &plan.https)?;
            let boundary = observe_http_boundary(&local, &plan.https)?;
            let local_inventory =
                observe_local_inventory(&local, &plan.https, (service_id, service_epoch))?;
            save_record(&plan.evidence().join("serve-during.json"), &during)?;
            save_record(
                &plan.evidence().join("host-ready.json"),
                &json!({"manifest":manifest,"inventory":inventory,"localInventory":local_inventory,"https":plan.https.url,"peerTls":"unobserved; requires the separate peer journey","httpBoundary":boundary,"ownedServeSession":session,"nativeActions":"none; endpoint/generation readiness only"}),
            )?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(DriverError::TimedOut("Host readiness deadline"))
}

fn run_projected(
    plan: &HostPlan,
    binary: &Path,
    native: &Path,
    before: &Value,
    profile: &Path,
) -> DriverResult<()> {
    let projection = ProfileProjection::acquire(profile, &plan.profile_sha256, plan.port)?;
    let body = (|| {
        let mut child = spawn_host(plan, binary, native)?;
        let image = wait_for_host_image(&mut child, binary);
        with_owned_child(&mut child, &plan.evidence(), |child, known| {
            image?;
            await_readiness(plan, child, known, before)?;
            let root =
                i32::try_from(child.id()).map_err(|_| DriverError::Runtime("Host PID overflow"))?;
            while !plan.evidence().join("stop.json").exists() {
                require_allocation(plan)?;
                if child.try_wait()?.is_some() {
                    return Err(DriverError::Runtime("Host exited without an explicit stop"));
                }
                observe_owned(root, known)?;
                require_protected(&plan.protected)?;
                std::thread::sleep(Duration::from_millis(200));
            }
            Ok(())
        })?;
        let settlement: Value = read_record(&plan.evidence().join("host-settlement.json"))?;
        if settlement.get("exit") != Some(&json!(0))
            || !settlement
                .get("signals")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        {
            return Err(DriverError::Runtime(
                "Host required forced/descendant cleanup or exited with failure",
            ));
        }
        Ok(())
    })();
    projection.restore()?;
    body
}

fn final_checks(
    plan: &HostPlan,
    before: &Value,
    profile: &Path,
    result: DriverResult<()>,
) -> DriverResult<()> {
    let failure = result
        .as_ref()
        .err()
        .map(|error| format!("{}: {error}", error.category()));
    let mut cleanup_errors = Vec::new();
    let serve = (|| {
        let after = serve_snapshot(&plan.tailscale)?;
        save_record(&plan.evidence().join("serve-after.json"), &after)?;
        require_serve_restored(before, &after)
    })();
    for check in [serve, require_protected(&plan.protected)] {
        if let Err(error) = check {
            cleanup_errors.push(format!("{}: {error}", error.category()));
        }
    }
    let restored = match sha256(profile) {
        Ok(hash) => hash == plan.profile_sha256,
        Err(_) => {
            cleanup_errors.push("debug profile became unreadable".to_owned());
            false
        }
    };
    save_record(
        &plan.evidence().join("lifecycle-settlement.json"),
        &json!({"failure":failure,"cleanupErrors":cleanup_errors,"profileRestoredExactly":restored}),
    )?;
    if !cleanup_errors.is_empty() {
        return Err(DriverError::Runtime(
            "lifecycle cleanup incomplete; inspect settlement",
        ));
    }
    result
}

pub fn run_host(plan: &HostPlan) -> DriverResult<()> {
    plan.validate()?;
    if rustix::process::geteuid().as_raw() == 0 {
        return Err(DriverError::Invalid(
            "the verification Host must remain unprivileged",
        ));
    }
    let user = nix::unistd::User::from_uid(nix::unistd::Uid::current())
        .map_err(|error| std::io::Error::from_raw_os_error(error as i32))?
        .ok_or(DriverError::Invalid("normal owner HOME unavailable"))?;
    let normal_home = user.dir;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| normal_home.clone());
    if home != normal_home {
        return Err(DriverError::Invalid("normal owner HOME must be preserved"));
    }
    validate_fresh_root(&plan.router_root)?;
    let binaries = qualify_artifacts(plan)?;
    require_protected(&plan.protected)?;
    if std::env::var_os("CODEX_HOME")
        .is_some_and(|value| value != normal_home.join(".codex").as_os_str())
    {
        return Err(DriverError::Invalid("normal Codex home must be preserved"));
    }
    drop(TcpListener::bind(("127.0.0.1", plan.port))?);
    let before = serve_snapshot(&plan.tailscale)?;
    if contains_port(&before, plan.https_port) {
        return Err(DriverError::Invalid("selected Serve port occupied"));
    }
    private_directory(&plan.router_root)?;
    private_directory(&plan.evidence())?;
    let native = plan.router_root.join("native");
    private_directory(&native)?;
    save_record(
        &plan.evidence().join("driver-binding.json"),
        &driver_binding(plan)?,
    )?;
    save_record(
        &plan.router_root.join("providers.json"),
        &json!({"version":1,"providers":{"claude":{"enabled":false,"executable":null,"arguments":[]},"cursor":{"enabled":false,"executable":null,"arguments":[]}}}),
    )?;
    save_record(&plan.evidence().join("serve-before.json"), &before)?;
    let profile = normal_home.join(".codex/codex-router-debug.config.toml");
    let binary = binaries
        .get("codex-router")
        .ok_or(DriverError::Invalid("qualified Router absent"))?;
    final_checks(
        plan,
        &before,
        &profile,
        run_projected(plan, binary, &native, &before, &profile),
    )
}
