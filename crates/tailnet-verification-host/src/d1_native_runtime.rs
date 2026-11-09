//! One real native-backed collaboration Host per process, using only D1-owned state.
use crate::{
    d1_central_runtime::D1MachineFixture,
    d1_host_lifecycle::{native_config, private_directory, stage_native, write_private},
    d1_host_plan::{
        D1AuthPolicy, D1NativeReadinessPlan, read_private_d1_record, require_service_directory,
        require_unaliased_ancestors,
    },
    driver_error::{DriverError, DriverResult},
    driver_records::{save_record, sha256},
    owned_host::with_owned_child,
    process_identity::lookup_identity,
};
use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use codex_native_integration::{
    AppServerCommandSpec, CodexPaths, CodexRouterProfile, DebugCodexProfile,
    NativeProtocolConnection, NativeSchemaExport, executable_identity,
};
use codex_router_host::{
    BackendSchemaEvidence, CollaborationRuntime, CollaborationRuntimeInputs, HostCoordinationPaths,
    HostInstance,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct D1HostPlan {
    pub native: D1NativeReadinessPlan,
    pub service_directory: PathBuf,
    pub fixture_file: PathBuf,
    pub reopen: bool,
}

fn boundary_error(error: impl std::fmt::Display) -> DriverError {
    DriverError::Io(std::io::Error::other(error.to_string()))
}

fn publication_time() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn wait_native_image(
    child: &mut std::process::Child,
    binary: &std::path::Path,
) -> DriverResult<()> {
    let pid =
        i32::try_from(child.id()).map_err(|_| DriverError::Invalid("D1 native PID overflow"))?;
    let expected = binary.canonicalize()?;
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait()?.is_some() {
            return Err(DriverError::Runtime("native exited before stable exec"));
        }
        if let Some(identity) = lookup_identity(pid)?
            && identity.group == pid
            && std::path::Path::new(&identity.executable) == expected
        {
            return Ok(());
        }
        if std::time::Instant::now() >= until {
            return Err(DriverError::TimedOut("native exec identity"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl D1HostPlan {
    fn validate(&self) -> DriverResult<()> {
        self.native.validate_allocation(!self.reopen)?;
        require_service_directory(&self.native.repository, &self.service_directory)?;
        require_unaliased_ancestors(&self.fixture_file)?;
        if !self
            .fixture_file
            .starts_with(self.native.repository.join("tmp/d1-verify/allocation"))
        {
            return Err(DriverError::Invalid(
                "D1 Host fixture is outside its allocation",
            ));
        }
        if !matches!(
            self.native.auth_policy,
            D1AuthPolicy::SharedDebugProxy { .. }
        ) {
            return Err(DriverError::Invalid(
                "D1 Host requires the declared shared debug proxy policy",
            ));
        }
        Ok(())
    }
}

fn session_inputs(fixture: &D1MachineFixture) -> DriverResult<BoardSessionInputs> {
    Ok(BoardSessionInputs {
        endpoint: fleet_session::Endpoint::new(&fixture.endpoint).map_err(boundary_error)?,
        access: AccessServiceToken::self_hosted(&fixture.assertion).map_err(boundary_error)?,
        credential: fleet_session::PeerCredential::new(fixture.credential.clone())
            .map_err(boundary_error)?,
        peer: fixture.peer.clone().try_into().map_err(boundary_error)?,
        receiver: fixture
            .receiver
            .clone()
            .try_into()
            .map_err(boundary_error)?,
        relay: fixture.relay.clone().try_into().map_err(boundary_error)?,
    })
}

async fn native_ready(
    socket: &std::path::Path,
    deadline: &chrono::DateTime<chrono::FixedOffset>,
) -> DriverResult<NativeProtocolConnection> {
    loop {
        if chrono::Utc::now() >= *deadline {
            return Err(DriverError::TimedOut("D1 native endpoint readiness"));
        }
        match NativeProtocolConnection::connect(socket).await {
            Ok(mut connection) => {
                let account = connection
                    .request("account/read", json!({"refreshToken":false}))
                    .await
                    .map_err(boundary_error)?;
                if account.get("requiresOpenaiAuth") != Some(&json!(false))
                    || account.get("account") != Some(&serde_json::Value::Null)
                {
                    return Err(DriverError::Runtime(
                        "D1 native unexpectedly uses client account auth",
                    ));
                }
                return Ok(connection);
            }
            Err(codex_native_integration::NativeConnectionError::Unavailable) => {
                // Same bounded connect backoff as the product AppServerChild readiness owner.
                // Readiness itself is the real initialize/account response, never this delay.
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(boundary_error(error)),
        }
    }
}

async fn serve(
    plan: &D1HostPlan,
    home: PathBuf,
    binary: PathBuf,
    backend: PathBuf,
) -> DriverResult<()> {
    let fixture: D1MachineFixture = read_private_d1_record(&plan.fixture_file)?;
    if fixture.service_directory != plan.service_directory {
        return Err(DriverError::Invalid(
            "D1 fixture belongs to another service directory",
        ));
    }
    let identity = executable_identity(&binary).await.map_err(boundary_error)?;
    let export_path = plan
        .native
        .run_root
        .join(format!("schema-{}", uuid::Uuid::now_v7()));
    let export = Arc::new(
        NativeSchemaExport::generate(&identity, &export_path)
            .await
            .map_err(boundary_error)?,
    );
    eprintln!("d1Startup nativeSchemaReady");
    let probe = native_ready(&backend, &plan.native.deadline).await?;
    eprintln!("d1Startup nativeProtocolReady");
    drop(probe);
    let peers = plan.native.run_root.join("peer-registry");
    if !peers.exists() {
        private_directory(&peers)?;
    }
    let instance = HostInstance::acquire(HostCoordinationPaths::new(
        plan.service_directory.join("h"),
        plan.native.run_root.join("host.lock"),
    ))
    .map_err(boundary_error)?;
    instance
        .clear_stale_endpoint_sockets(&plan.service_directory)
        .await?;
    eprintln!("d1Startup collaborationStartBegin");
    let mut runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: Some(session_inputs(&fixture)?),
        owner_human_id: None,
        directory: plan.service_directory.clone(),
        codex_home: home,
        backend_socket: backend,
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        native_schema: Some(export.clone()),
        peer_registry_directory: Some(peers),
        remote_control_server_name: None,
    })
    .await?;
    eprintln!("d1Startup collaborationStartReady");
    let driven = async {
        eprintln!("d1Startup backendPublicationBegin");
        runtime.backend_ready(publication_time().try_into().map_err(boundary_error)?,
            Some(BackendSchemaEvidence { running_executable: &identity, export: &export })).await?;
        eprintln!("d1Startup backendPublicationReady");
        let service_id = String::from(runtime.service_id().clone());
        let epoch = String::from(runtime.service_epoch().clone());
        if service_id != fixture.service_id || runtime.installation_peer().to_string() != fixture.peer {
            return Err(DriverError::Runtime("D1 Host identity differs from authoritative enrollment"));
        }
        let ready = json!({"kind":"d1HostReady","pid":std::process::id(),"serviceDirectory":plan.service_directory,
            "serviceId":fixture.service_id,"peer":fixture.peer,"epoch":epoch,
            "nativeSHA256":sha256(&binary)?,"modelInputSent":false});
        save_record(&plan.native.run_root.join(format!("host-ready-{epoch}.json")), &ready)?;
        println!("{ready}");
        std::io::stdout().flush()?;
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let remaining = (plan.native.deadline.with_timezone(&chrono::Utc) - chrono::Utc::now()).to_std()
            .map_err(|_| DriverError::TimedOut("D1 Host deadline"))?;
        tokio::select! {
            line = lines.next_line() => { let _line = line?; }
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
            failure = runtime.listener_failure() => return Err(boundary_error(failure)),
            _ = tokio::time::sleep(remaining) => return Err(DriverError::TimedOut("D1 Host allocation expired")),
        }
        Ok(())
    }.await;
    let stopped = runtime.shutdown().await;
    drop(instance);
    stopped?;
    driven?;
    println!(
        "{}",
        json!({"kind":"d1HostStopped","serviceDirectory":plan.service_directory})
    );
    Ok(())
}

#[cfg(test)]
#[path = "d1_native_runtime_test.rs"]
mod tests;

pub fn run(plan: D1HostPlan) -> DriverResult<()> {
    plan.validate()?;
    let (home, binary) = if plan.reopen {
        let home = plan.native.run_root.join("codex");
        let binary = home.join("packages/standalone/current/codex");
        if sha256(&binary)? != plan.native.native.sha256 {
            return Err(DriverError::Invalid("D1 reopened native bytes changed"));
        }
        (home, binary)
    } else {
        stage_native(&plan.native)?
    };
    let D1AuthPolicy::SharedDebugProxy { origin } = &plan.native.auth_policy else {
        return Err(DriverError::Invalid("shared debug proxy policy required"));
    };
    let port = reqwest::Url::parse(origin)
        .map_err(boundary_error)?
        .port()
        .ok_or(DriverError::Invalid("debug proxy port absent"))?;
    if !plan.reopen {
        write_private(&home.join("config.toml"), native_config(port).as_bytes())?;
    }
    let native_directory = plan.service_directory.join("n");
    if !native_directory.exists() {
        private_directory(&native_directory)?;
    }
    let backend = native_directory.join("s");
    std::os::unix::net::SocketAddr::from_pathname(&backend)?;
    if std::env::var_os("CODEX_HOME").as_deref() != Some(home.as_os_str()) {
        return Err(DriverError::Invalid(
            "D1 controller must set its allocated CODEX_HOME",
        ));
    }
    let lifetime_evidence = plan
        .native
        .run_root
        .join(format!("lifetime-{}", uuid::Uuid::now_v7()));
    private_directory(&lifetime_evidence)?;
    let executor = tokio::runtime::Runtime::new()?;
    let native_log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(
            plan.native
                .run_root
                .join(format!("native-{}.stderr", uuid::Uuid::now_v7())),
        )?;
    let profile = native_config(port).replace("cli_auth_credentials_store = \"ephemeral\"\n", "");
    let profile = DebugCodexProfile::parse(&profile, port).map_err(boundary_error)?;
    let spec = AppServerCommandSpec::new(
        &CodexPaths::from_codex_home(home.clone()),
        &CodexRouterProfile::new(port),
        &backend,
    )
    .with_debug_profile(&profile);
    let mut child = Command::new(std::env::current_exe()?)
        .arg("__launch-host")
        .arg(&binary)
        .args(spec.arguments())
        .env_clear()
        .env(
            "HOME",
            std::env::var_os("HOME").ok_or(DriverError::Invalid("HOME unavailable"))?,
        )
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("CODEX_HOME", &home)
        .env("TMPDIR", &plan.native.run_root)
        .env("CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED", "1")
        .current_dir(&plan.native.run_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(native_log)
        .spawn()?;
    let native_image = wait_native_image(&mut child, &binary);
    with_owned_child(&mut child, &lifetime_evidence, |_child, _known| {
        native_image?;
        executor.block_on(serve(&plan, home, binary, backend))
    })
}
