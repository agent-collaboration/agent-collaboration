//! Same-run compiled UI journeys. Inventory is synthetic; input and rendering are real.
#[path = "cua_artifact_sink.rs"]
mod cua_artifact_sink;
#[path = "public_sessions_capture.rs"]
mod public_sessions_capture;

use super::provider_filter_tape::{
    build_compact_filter_tape, build_filter_tape, validate_compact_checkpoints,
};
use super::*;
use crate::isolated_fixture_test::ProviderFilterFixtureScenario;

#[derive(Clone, Copy)]
pub(crate) enum ProviderFilterJourney {
    QuotaMixed,
    QuotaEmpty,
    Sessions,
    CompactSessions,
    CompactQuota,
}

pub(crate) async fn run_provider_filter_storyboard(
    columns: u16,
    rows: u16,
    journey: ProviderFilterJourney,
) -> TestResult<()> {
    if let Some(surface) = std::env::var_os("QUOTA_STORYBOARD_CUA_SURFACE") {
        let requested_surface = match surface.to_str() {
            Some("sessions") => CapturedTuiSurface::Sessions,
            Some("quota") => CapturedTuiSurface::Quota,
            _ => return Err("CUA surface must be sessions or quota".into()),
        };
        let selected = matches!(
            (requested_surface, journey),
            (
                CapturedTuiSurface::Sessions,
                ProviderFilterJourney::CompactSessions
            ) | (
                CapturedTuiSurface::Quota,
                ProviderFilterJourney::CompactQuota
            )
        );
        // This existing test's caller loops over static captures. The explicit
        // CUA mode leases one live fixture instead; it earns no static PNG proof.
        if selected && columns == 120 && rows == 36 {
            run_live_cua_fixture(requested_surface).await?;
        }
        return Ok(());
    }
    // The caller declares each static grid exactly once, including 48x12.
    // The CUA branch above still leases only the single 120x36 starting PTY.
    run_provider_filter_capture(columns, rows, journey).await
}

struct QuotaComponentFixture {
    binary: PathBuf,
    scenario: String,
}

impl QuotaComponentFixture {
    fn from_environment(surface: CapturedTuiSurface) -> TestResult<Option<Self>> {
        let binary = std::env::var_os("QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY");
        let scenario = std::env::var("QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO");
        match (binary, scenario) {
            (None, Err(std::env::VarError::NotPresent)) => Ok(None),
            (Some(binary), Ok(scenario)) => {
                ensure(
                    matches!(surface, CapturedTuiSurface::Quota),
                    "component fixture is quota only",
                )?;
                ensure(
                    matches!(
                        scenario.as_str(),
                        "normal"
                            | "inspection-pending"
                            | "commit-pending"
                            | "unknown-result"
                            | "save-failed"
                            | "disabled-yes"
                            | "revalidation-pending"
                            | "refused-result"
                            | "refresh-pending"
                            | "save-pending"
                            | "visual-hard-cases"
                    ),
                    "unknown component fixture scenario",
                )?;
                let binary = fs::canonicalize(binary)?;
                ensure(binary.is_file(), "component fixture binary missing")?;
                Ok(Some(Self { binary, scenario }))
            }
            _ => Err("component fixture requires exact scenario and binary".into()),
        }
    }
}

async fn run_live_cua_fixture(surface: CapturedTuiSurface) -> TestResult<()> {
    let (_, node_sha256) = verified_node_executable()?;
    if std::env::var_os("QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG").is_some() {
        ensure(
            matches!(surface, CapturedTuiSurface::Sessions),
            "public Sessions is Sessions-only",
        )?;
        return public_sessions_capture::run_live_public_sessions().await;
    }
    let tools = CaptureTools::from_environment()?;
    let output_root = PathBuf::from(
        std::env::var_os("QUOTA_STORYBOARD_OUTPUT").ok_or("fresh CUA output root required")?,
    );
    let output = fs::canonicalize(output_root)?.join(format!("cua-{}", surface.label()));
    fs::create_dir(&output)?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700))?;
    let fixture =
        QuotaResetFixture::create_provider_filters(ProviderFilterFixtureScenario::MixedAccounts)
            .await?;
    fixture.assert_read_only()?;
    let provider = HeldLoopbackProvider::bind()?;
    let component_fixture = QuotaComponentFixture::from_environment(surface)?;
    let binary = match &component_fixture {
        Some(component) => component.binary.clone(),
        None => fs::canonicalize(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness"))?,
    };
    tools.write_fixed_child_adapter(&output, &binary, &fixture, provider.address(), surface)?;
    if let Some(component) = &component_fixture {
        fs::write(
            output.join("fixture-command.json"),
            format!(
                "{{\"binary\":{},\"output\":{},\"temporary_directory\":{},\"surface\":\"quota-component\",\"component_fixture_scenario\":{},\"component_fixture_receipt\":{},\"component_fixture_release\":{}}}\n",
                encode_json_string(&binary.to_string_lossy()),
                encode_json_string(&output.to_string_lossy()),
                encode_json_string(&std::env::temp_dir().to_string_lossy()),
                encode_json_string(&component.scenario),
                encode_json_string(&output.join("fixture-finished").to_string_lossy()),
                encode_json_string(&output.join("fixture-settlement").to_string_lossy()),
            ),
        )?;
    }
    let scenario_json = component_fixture.as_ref().map_or_else(
        || "null".to_owned(),
        |component| encode_json_string(&component.scenario),
    );
    let scenes = std::env::var("QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES")
        .map_err(|_| "CUA needs predeclared original-artifact scenes")?
        .split(',')
        .map(str::to_owned)
        .collect();
    let artifact_sink = cua_artifact_sink::CuaArtifactSink::start(&output, scenes).await?;
    let artifact_export = artifact_sink.readiness_json();
    let result: TestResult<()> = async {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    let browser_temporary_directory = create_browser_temporary_directory(&output)?;
    let child = Command::new(&tools.ttyd)
        .arg(format!("--port={}", address.port()))
        .args([
            "--interface",
            "127.0.0.1",
            "--once",
            "--writable",
            "-t",
            "rendererType=canvas",
            "-t",
            "disableResizeOverlay=true",
            "-t",
            "fontFamily=Menlo",
            "-t",
            "fontSize=16",
        ])
        .arg(verified_node_executable()?.0)
        .arg(permanent_adapter_source("fixture-child.ts")?)
        .arg(output.join("fixture-command.json"))
        .current_dir(&output)
        .env_clear()
        .env("HOME", &output)
        .env("TMPDIR", &browser_temporary_directory)
        .env("LANG", "C.UTF-8")
        .env("TERM", "xterm-256color")
        .env("DYLD_LIBRARY_PATH", &tools.dynamic_library_path)
        .env(
            "QUOTA_STORYBOARD_FIXTURE_COMMAND",
            output.join("fixture-command.json"),
        )
        .stdout(fs::File::create(output.join("cua-ttyd.stdout"))?)
        .stderr(fs::File::create(output.join("cua-ttyd.stderr"))?)
        .process_group(0)
        .spawn();
    let child = match child {
        Ok(child) => child,
        Err(error) => {
            fs::remove_dir_all(&browser_temporary_directory)?;
            return Err(error.into());
        }
    };
    let mut capture = OwnedCapture {
        child,
        group_released: false,
        output: output.clone(),
        child_ownership: if component_fixture.is_some() {
            CapturedChildOwnership::RetainedAdapterChild
        } else {
            CapturedChildOwnership::FixtureCapability(fixture.capability().to_owned())
        },
        browser_temporary_directory,
        stop_file: None,
    };
    fs::write(output.join("ttyd.pid"), capture.child.id().to_string())?;
    let readiness_deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        ensure(
            capture.child.try_wait()?.is_none(),
            "owned ttyd exited before readiness",
        )?;
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            break;
        }
        ensure(
            std::time::Instant::now() < readiness_deadline,
            "owned ttyd listener not ready",
        )?;
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    fs::write(
        output.join("cua-ready.json"),
        format!(
            "{{\"kind\":\"listener-ready-not-ui\",\"surface\":{},\"url\":{},\"ttyd_pid\":{},\"binary_sha256\":{},\"node_sha256\":\"{node_sha256}\",\"ttyd_sha256\":{},\"child_pid_file\":{},\"viewport_file\":{},\"stop_file\":{},\"lifetime_seconds\":600,\"quota_fixture_scenario\":{scenario_json},\"artifact_export\":{artifact_export}}}\n",
            encode_json_string(surface.label()),
            encode_json_string(&format!("http://127.0.0.1:{}/", address.port())),
            capture.child.id(),
            encode_json_string(&digest_file(&binary)?),
            encode_json_string(&digest_file(&tools.ttyd)?),
            encode_json_string(&output.join("quota-child.pid").to_string_lossy()),
            encode_json_string(&output.join("quota-child.viewport").to_string_lossy()),
            encode_json_string(&output.join("cua-stop").to_string_lossy()),
        ),
    )?;
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    let status = loop {
        if let Some(status) = capture.child.try_wait()? {
            break status;
        }
        if output.join("cua-stop").exists() {
            capture.release_process_group()?;
            return Err("CUA stopped before natural child settlement; retain evidence".into());
        }
        ensure(
            std::time::Instant::now() < deadline,
            "live CUA fixture reached its deadline",
        )?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    capture.release_process_group()?;
    ensure(status.success(), "live CUA ttyd exited unsuccessfully")?;
    ensure_owned_child_stopped(&output)?;
    ensure(
        fs::read_to_string(output.join("quota-child.exit"))?.trim() == "0",
        "live CUA child failed",
    )?;
    ensure(
        provider.finish()?.is_empty(),
        "live CUA journey contacted provider",
    )?;
    fixture.assert_read_only()?;
    let fixture_root = fixture.root().to_owned();
    drop(fixture);
    ensure(!fixture_root.exists(), "live CUA fixture survived cleanup")?;
    Ok(())
    }.await;
    let sink_result = artifact_sink.stop().await;
    result?;
    let artifact_count = sink_result?;
    verified_node_executable()?;
    permanent_adapter_source("fixture-child.ts")?;
    permanent_adapter_source("ttyd-adapter.ts")?;
    fs::write(
        output.join("cua-receipt.json"),
        format!(
            "{{\"kind\":\"live-cua-fixture-lifecycle-only\",\"surface\":{},\"binary_sha256\":{},\"node_sha256\":\"{node_sha256}\",\"child_exit\":0,\"provider_requests\":0,\"state_and_secrets_unchanged\":true,\"owned_processes_stopped\":true,\"artifact_sink_stopped\":true,\"artifact_count\":{artifact_count},\"last_viewport\":{},\"ui_observation\":\"separate-original-CUA-evidence-required\",\"quota_fixture_scenario\":{scenario_json}}}\n",
            encode_json_string(surface.label()),
            encode_json_string(&digest_file(&binary)?),
            encode_json_string(fs::read_to_string(output.join("quota-child.viewport"))?.trim()),
        ),
    )?;
    Ok(())
}

async fn run_provider_filter_capture(
    columns: u16,
    rows: u16,
    journey: ProviderFilterJourney,
) -> TestResult<()> {
    let sessions = matches!(
        journey,
        ProviderFilterJourney::Sessions | ProviderFilterJourney::CompactSessions
    );
    let compact = matches!(
        journey,
        ProviderFilterJourney::CompactSessions | ProviderFilterJourney::CompactQuota
    );
    let empty_quota = matches!(journey, ProviderFilterJourney::QuotaEmpty);
    let surface = if sessions {
        CapturedTuiSurface::Sessions
    } else {
        CapturedTuiSurface::Quota
    };
    let tools = CaptureTools::from_environment()?;
    ensure_capture_running(tools.stop_file.as_deref())?;
    let output_root = PathBuf::from(
        std::env::var_os("QUOTA_STORYBOARD_OUTPUT")
            .ok_or("fresh filter storyboard output root required")?,
    );
    fs::create_dir_all(&output_root)?;
    let output = fs::canonicalize(output_root)?.join(format!(
        "{}-{}-{columns}x{rows}",
        surface.label(),
        if compact {
            "compact"
        } else if empty_quota {
            "empty"
        } else {
            "filters"
        }
    ));
    fs::create_dir(&output)?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700))?;
    let fixture = QuotaResetFixture::create_provider_filters(if empty_quota {
        ProviderFilterFixtureScenario::ClaudeAccountsDisabled
    } else {
        ProviderFilterFixtureScenario::MixedAccounts
    })
    .await?;
    fixture.assert_read_only()?;
    let provider = HeldLoopbackProvider::bind()?;
    let binary = fs::canonicalize(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness"))?;
    tools.write_fixed_child_adapter(&output, &binary, &fixture, provider.address(), surface)?;
    let (tape, checkpoints) = if compact {
        build_compact_filter_tape(surface, columns, rows)
    } else {
        build_filter_tape(surface, columns, rows, empty_quota)
    };
    if compact {
        validate_compact_checkpoints(surface, columns, rows, &checkpoints)?;
    }
    fs::write(output.join("quota.tape"), tape)?;
    let mut capture = tools.start_capture(&output, fixture.capability())?;
    let status = capture.wait(Duration::from_secs(90))?;
    if !status.success() {
        record_failed_child_state(&output, &binary, fixture.capability())?;
    }
    capture.release_process_group()?;
    ensure(
        status.success(),
        "filter capture failed: inspect owned VHS stdout/stderr",
    )?;
    ensure_owned_child_stopped(&output)?;
    ensure(
        fs::read_to_string(output.join("quota-child.viewport"))? == format!("{columns} {rows}"),
        "filter child viewport mismatch",
    )?;
    ensure(
        fs::read_to_string(output.join("quota-child.exit"))?.trim() == "0",
        "filter child failed",
    )?;
    ensure(
        provider.finish()?.is_empty(),
        "filtering contacted the provider",
    )?;
    fixture.assert_read_only()?;
    let transcript = fs::read_to_string(output.join("screens.txt"))?;
    for canary in FORBIDDEN_TERMINAL_CANARIES {
        ensure(
            !transcript.to_lowercase().contains(canary),
            "filter screen leaked credential canary",
        )?;
    }
    ensure(!transcript.contains("panicked at"), "filter TUI panicked")?;
    if sessions {
        ensure(
            transcript.contains("SESSION_PICKER_OUTCOME canceled"),
            "session journey must exit canceled without launching any session",
        )?;
    }
    for checkpoint in &checkpoints {
        let png = output.join(format!("{checkpoint}.png"));
        validate_png(&png)?;
        tools.verify_image(&output, checkpoint, &png, false)?;
    }
    let blank = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/quota_storyboard_capture_test/fixtures/blank-startup.png"
    ));
    let (expected, wrong, committed, chooser) = if compact {
        if sessions {
            (
                "compact-sessions-claude",
                "compact-sessions-all",
                "compact-sessions-cancelled",
                "compact-sessions-chooser",
            )
        } else {
            (
                "compact-quota-claude",
                "compact-quota-all",
                "compact-quota-cancelled",
                "compact-quota-chooser",
            )
        }
    } else if sessions {
        (
            "sessions-codex",
            "sessions-claude",
            "sessions-draft",
            "sessions-chooser",
        )
    } else {
        (
            "quota-openai",
            "quota-claude",
            "quota-draft",
            "quota-chooser",
        )
    };
    tools.verify_image(&output, expected, blank, true)?;
    if !empty_quota {
        tools.verify_image(
            &output,
            expected,
            &output.join(format!("{wrong}.png")),
            true,
        )?;
        tools.verify_image(
            &output,
            committed,
            &output.join(format!("{chooser}.png")),
            true,
        )?;
    }
    if compact && matches!(surface, CapturedTuiSurface::Quota) {
        // Same order as quotaNegativePairs in scripts/verification/tui/scene-contracts.ts.
        for (checkpoint, wrong) in [
            (
                "compact-quota-hidden-choice",
                "compact-quota-disabled-choice",
            ),
            // C6: F5 must open Pool details, not leave browse unchanged...
            ("compact-quota-pool-details", "compact-quota-all"),
            //...and Escape must actually close it.
            ("compact-quota-pool-closed", "compact-quota-pool-details"),
        ] {
            tools.verify_image(
                &output,
                checkpoint,
                &output.join(format!("{wrong}.png")),
                true,
            )?;
        }
    }
    if compact && sessions {
        for checkpoint in checkpoints
            .iter()
            .filter(|name| name.starts_with("compact-sessions-details"))
        {
            tools.verify_image(
                &output,
                checkpoint,
                &output.join("compact-sessions-codex-details.png"),
                true,
            )?;
        }
        if columns == 48 && rows == 12 {
            tools.verify_image(
                &output,
                "compact-sessions-details-identity",
                &output.join("compact-sessions-details-title.png"),
                true,
            )?;
        }
    }
    write_filter_receipt(
        &output,
        &binary,
        &tools,
        columns,
        rows,
        surface,
        &checkpoints,
    )?;
    let fixture_root = fixture.root().to_owned();
    drop(fixture);
    ensure(!fixture_root.exists(), "filter fixture survived cleanup")?;
    println!("compiled filter storyboard verified: {}", output.display());
    Ok(())
}

// Capture a bounded stack observation before cleanup destroys the evidence. This
// does not retry, send input, redraw, or change the application's startup path.
fn record_failed_child_state(output: &Path, binary: &Path, capability: &str) -> TestResult<()> {
    let record = output.join("quota-child.pid");
    if !record.exists() {
        return Ok(());
    }
    let pid: u32 = fs::read_to_string(record)?.trim().parse()?;
    let snapshot = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()?;
    fs::write(output.join("failed-child-command.txt"), &snapshot.stdout)?;
    if !snapshot.status.success() {
        return Ok(());
    }
    let command = String::from_utf8_lossy(&snapshot.stdout);
    ensure(
        command.contains(binary.to_string_lossy().as_ref()) && command.contains(capability),
        "refusing to sample a process outside this exact owned fixture",
    )?;
    let observation = Command::new("/usr/bin/sample")
        .args([&pid.to_string(), "1", "1", "-file"])
        .arg(output.join("failed-child.sample.txt"))
        .output()?;
    fs::write(
        output.join("failed-child-sample.stdout"),
        observation.stdout,
    )?;
    fs::write(
        output.join("failed-child-sample.stderr"),
        observation.stderr,
    )?;
    Ok(())
}

fn write_filter_receipt(
    output: &Path,
    binary: &Path,
    tools: &CaptureTools,
    columns: u16,
    rows: u16,
    surface: CapturedTuiSurface,
    checkpoints: &[&str],
) -> TestResult<()> {
    let (_, node_sha256) = verified_node_executable()?;
    let images = checkpoints
        .iter()
        .map(|name| {
            Ok(format!(
                "{{\"checkpoint\":{},\"png_sha256\":{}}}",
                encode_json_string(name),
                encode_json_string(&digest_file(&output.join(format!("{name}.png")))?)
            ))
        })
        .collect::<TestResult<Vec<_>>>()?
        .join(",");
    let verifier_source = format!(
        "{:x}",
        Sha256::digest(include_bytes!("verify_screen_image.swift"))
    );
    let verifier_checkpoint_source = format!(
        "{:x}",
        Sha256::digest(include_bytes!("verify_screen_checkpoint.swift"))
    );
    let tape_driver_source = format!(
        "{:x}",
        Sha256::digest(include_bytes!("provider_filter_tape.rs"))
    );
    let driver_source = format!(
        "{:x}",
        Sha256::digest(include_bytes!("provider_filter_capture.rs"))
    );
    fs::write(
        output.join("filter-receipt.json"),
        format!(
            "{{\"surface\":{},\"columns\":{columns},\"rows\":{rows},\"binary_sha256\":{},\"node_sha256\":\"{node_sha256}\",\"vhs_sha256\":{},\"ttyd_sha256\":{},\"verifier_sha256\":{},\"verifier_source_sha256\":{},\"verifier_checkpoint_source_sha256\":{},\"driver_source_sha256\":{},\"tape_driver_source_sha256\":{},\"fixed_child_sha256\":{},\"tape_sha256\":{},\"state_and_secrets_unchanged\":true,\"provider_requests\":0,\"child_exit\":0,\"owned_processes_stopped\":true,\"fixture\":\"synthetic typed inputs; actual compiled UI/input/rendering\",\"images\":[{images}]}}\n",
            encode_json_string(surface.label()),
            encode_json_string(&digest_file(binary)?),
            encode_json_string(&digest_file(&tools.vhs)?),
            encode_json_string(&digest_file(&tools.ttyd)?),
            encode_json_string(&digest_file(&tools.image_verifier)?),
            encode_json_string(&verifier_source),
            encode_json_string(&verifier_checkpoint_source),
            encode_json_string(&driver_source),
            encode_json_string(&tape_driver_source),
            encode_json_string(&digest_file(&permanent_adapter_source(
                "fixture-child.ts"
            )?)?),
            encode_json_string(&digest_file(&output.join("quota.tape"))?)
        ),
    )?;
    Ok(())
}
