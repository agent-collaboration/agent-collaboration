//! Public launch/effect boundary over the existing ttyd and original-JPEG sink.
use super::cua_artifact_sink::CuaArtifactSink;
use super::*;

fn required_public_value(name: &str) -> TestResult<String> {
    let value = std::env::var(name)?;
    ensure(!value.is_empty(), "missing public target value")?;
    Ok(value)
}

pub(super) async fn run_live_public_sessions() -> TestResult<()> {
    ensure(
        std::env::var_os("QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY").is_none(),
        "public and synthetic component variants cannot be mixed",
    )?;
    let tools = CaptureTools::from_environment()?;
    let root = fs::canonicalize(
        std::env::var_os("QUOTA_STORYBOARD_OUTPUT").ok_or("fresh public output required")?,
    )?;
    let output = root.join("cua-sessions");
    let config = fs::canonicalize(required_public_value(
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG",
    )?)?;
    ensure(
        config == root.join("public-target.json"),
        "public configuration must belong to this output",
    )?;
    ensure(
        digest_file(&config)?
            == required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG_SHA256")?,
        "public configuration binding changed",
    )?;
    let binary = fs::canonicalize(required_public_value(
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_BINARY",
    )?)?;
    ensure(binary.is_file(), "public binary missing")?;
    let service = required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_ID")?;
    let epoch = required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_EPOCH")?;
    let effects = required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_EFFECT_SHA256")?;
    ensure(
        effects.len() == 64 && effects.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "public effect proof binding required",
    )?;
    fs::create_dir(&output)?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700))?;
    tools.write_child_adapter_files(&output)?;
    fs::copy(&config, output.join("fixture-command.json"))?;
    let scenes = required_public_value("QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES")?
        .split(',')
        .map(str::to_owned)
        .collect();
    let sink = CuaArtifactSink::start(&output, scenes).await?;
    let export = sink.readiness_json();
    let result: TestResult<()> = async {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        drop(listener);
        let browser_directory = create_browser_temporary_directory(&output)?;
        // Preserve actual inherited environment, including HOME/CODEX_HOME absence.
        // The fixed Python adapter rechecks public argv/hash/cwd/Host before Popen.
        let child = Command::new(&tools.ttyd)
            .arg(format!("--port={}", address.port()))
            .args(["--interface", "127.0.0.1", "--once", "--writable", "-t", "rendererType=canvas", "-t", "disableResizeOverlay=true", "-t", "fontFamily=Menlo", "-t", "fontSize=16"])
            .arg("/usr/bin/python3").arg(output.join("fixed_quota_child.py"))
            .current_dir(&output)
            .env("TMPDIR", &browser_directory).env("LANG", "C.UTF-8").env("TERM", "xterm-256color")
            .env("DYLD_LIBRARY_PATH", &tools.dynamic_library_path)
            .env("QUOTA_STORYBOARD_FIXTURE_COMMAND", output.join("fixture-command.json"))
            .stdout(fs::File::create(output.join("cua-ttyd.stdout"))?)
            .stderr(fs::File::create(output.join("cua-ttyd.stderr"))?)
            .process_group(0).spawn();
        let child = match child {
            Ok(child) => child,
            Err(error) => { fs::remove_dir_all(&browser_directory)?; return Err(error.into()); }
        };
        let mut capture = OwnedCapture { child, group_released: false, output: output.clone(),
            child_ownership: CapturedChildOwnership::RetainedPythonChild,
            browser_temporary_directory: browser_directory, stop_file: None };
        fs::write(output.join("ttyd.pid"), capture.child.id().to_string())?;
        let ready_deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            ensure(capture.child.try_wait()?.is_none(), "public ttyd exited before readiness")?;
            if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() { break; }
            ensure(std::time::Instant::now() < ready_deadline, "public ttyd listener unavailable")?;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        fs::write(output.join("cua-ready.json"), format!(
            "{{\"kind\":\"listener-ready-not-ui\",\"surface\":\"sessions\",\"url\":{},\"ttyd_pid\":{},\"binary_sha256\":{},\"ttyd_sha256\":{},\"child_pid_file\":{},\"viewport_file\":{},\"stop_file\":{},\"lifetime_seconds\":600,\"quota_fixture_scenario\":null,\"artifact_export\":{export}}}\n",
            encode_json_string(&format!("http://127.0.0.1:{}/", address.port())), capture.child.id(),
            encode_json_string(&digest_file(&binary)?), encode_json_string(&digest_file(&tools.ttyd)?),
            encode_json_string(&output.join("quota-child.pid").to_string_lossy()),
            encode_json_string(&output.join("quota-child.viewport").to_string_lossy()),
            encode_json_string(&output.join("cua-stop").to_string_lossy()),
        ))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(600);
        loop {
            if let Some(status) = capture.child.try_wait()? {
                ensure(status.success(), "public ttyd failed")?;
                break;
            }
            if output.join("cua-stop").exists() {
                capture.release_process_group()?;
                return Err("public CUA stopped early; job state remains with runtime helper".into());
            }
            ensure(std::time::Instant::now() < deadline, "public CUA lifetime elapsed")?;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        capture.release_process_group()?;
        ensure_owned_child_stopped(&output)?;
        ensure(fs::read_to_string(output.join("quota-child.exit"))?.trim() == "0", "public child failed")?;
        Ok(())
    }.await;
    let stopped = sink.stop().await;
    result?;
    let count = stopped?;
    fs::write(
        output.join("public-cua-receipt.json"),
        format!(
            "{{\"kind\":\"public-sessions-cua-lifecycle-only\",\"binary_sha256\":{},\"public_capture_source_sha256\":{},\"fixed_child_sha256\":{},\"host_service_id\":{},\"host_service_epoch\":{},\"child_exit\":0,\"owned_processes_stopped\":true,\"artifact_sink_stopped\":true,\"artifact_count\":{count},\"last_viewport\":{},\"effect_evidence_sha256\":{},\"job_settlement\":\"separate-runtime-helper-proof-required\"}}\n",
            encode_json_string(&digest_file(&binary)?),
            encode_json_string(&format!(
                "{:x}",
                Sha256::digest(include_bytes!("public_sessions_capture.rs"))
            )),
            encode_json_string(&digest_file(&output.join("fixed_quota_child.py"))?),
            encode_json_string(&service),
            encode_json_string(&epoch),
            encode_json_string(fs::read_to_string(output.join("quota-child.viewport"))?.trim()),
            encode_json_string(&effects),
        ),
    )?;
    Ok(())
}
