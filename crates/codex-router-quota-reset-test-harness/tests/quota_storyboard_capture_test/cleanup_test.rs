use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use super::CaptureTools;
use super::HeldLoopbackProvider;
use super::QuotaResetFixture;
use super::TestResult;
use super::ensure;
use super::ensure_owned_child_stopped;

#[tokio::test(flavor = "current_thread")]
async fn owned_capture_cleanup_scopes_the_group_operand_and_preserves_unrelated_processes()
-> TestResult<()> {
    // Check the real command owner before starting any child: a regression
    // must fail without executing a potentially process-wide signal.
    let command = super::owned_capture::owned_capture_group_termination_command(12345);
    ensure(
        command.get_program() == "/bin/kill"
            && command.get_args().map(OsString::from).collect::<Vec<_>>()
                == ["-TERM", "--", "-12345"].map(OsString::from),
        "owned group signal must separate kill options from its negative group operand",
    )?;

    let fixture = QuotaResetFixture::create().await?;
    let output = fixture.root().join("group-signal-cleanup");
    fs::create_dir(&output)?;
    let vhs = fixture.root().join("controlled-group-child");
    fs::write(&vhs, "#!/bin/sh\nexec /bin/sleep 30\n")?;
    fs::set_permissions(&vhs, fs::Permissions::from_mode(0o700))?;
    let tools = CaptureTools {
        vhs,
        ttyd: PathBuf::new(),
        image_verifier: PathBuf::new(),
        dynamic_library_path: String::new(),
        image_timeout: Duration::from_secs(1),
        stop_file: None,
    };
    let mut sentinel = Command::new("/bin/sleep")
        .arg("30")
        .process_group(0)
        .spawn()?;
    let outcome = (|| -> TestResult<()> {
        let mut capture = tools.start_capture(&output, fixture.capability())?;
        ensure(
            capture.child.id() != sentinel.id(),
            "capture and sentinel must be different process-group leaders",
        )?;
        let browser_directory = capture.browser_temporary_directory.clone();
        capture.release_process_group()?;
        let deadline = Instant::now() + Duration::from_secs(1);
        let capture_status = loop {
            if let Some(status) = capture.child.try_wait()? {
                break status;
            }
            ensure(
                Instant::now() < deadline,
                "owned capture survived group cleanup",
            )?;
            std::thread::sleep(Duration::from_millis(5));
        };
        ensure(
            capture_status.signal() == Some(15),
            "real owned capture must terminate through group cleanup",
        )?;
        ensure(
            sentinel.try_wait()?.is_none(),
            "unrelated process group must remain alive after capture cleanup",
        )?;
        ensure(
            !browser_directory.exists() && !output.join("cleanup-error.txt").exists(),
            "group cleanup must retain owned-profile and cleanup-error checks",
        )?;
        fixture.assert_read_only()?;
        Ok(())
    })();
    // Positive-PID Child ownership settles our sentinel even on assertion failure.
    let sentinel_cleanup = (|| -> TestResult<()> {
        if sentinel.try_wait()?.is_none() {
            sentinel.kill()?;
        }
        sentinel.wait()?;
        Ok(())
    })();
    outcome?;
    sentinel_cleanup
}

#[tokio::test(flavor = "current_thread")]
async fn failed_tool_spawn_removes_its_owned_browser_directory() -> TestResult<()> {
    let fixture = QuotaResetFixture::create().await?;
    let tools = CaptureTools {
        vhs: fixture.root().join("absent-vhs-tool"),
        ttyd: PathBuf::new(),
        image_verifier: PathBuf::new(),
        dynamic_library_path: String::new(),
        image_timeout: Duration::from_secs(30),
        stop_file: None,
    };
    ensure(
        tools
            .start_capture(fixture.root(), fixture.capability())
            .is_err(),
        "missing capture tool must fail without launching a child",
    )?;
    let directory = fs::read_to_string(fixture.root().join("browser-temp.directory"))?;
    ensure(
        !std::path::Path::new(directory.trim()).exists(),
        "failed tool spawn leaked its owned browser directory",
    )?;
    fixture.assert_read_only()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn hung_image_checker_returns_before_watchdog_and_retains_failure_output() -> TestResult<()> {
    controlled_image_lifecycle(false).await
}

#[tokio::test(flavor = "current_thread")]
async fn private_stop_interrupts_an_active_image_child_and_retains_failure_output() -> TestResult<()>
{
    controlled_image_lifecycle(true).await
}

async fn controlled_image_lifecycle(cooperative_stop: bool) -> TestResult<()> {
    let fixture = QuotaResetFixture::create().await?;
    let root = fixture.root().to_path_buf();
    let fifo = root.join("checker-release");
    ensure(
        Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()?
            .success(),
        "controlled checker FIFO must exist",
    )?;
    let checker = root.join("controlled-image-checker");
    let pid_file = root.join("checker.pid");
    let stop_file = root.join("capture-stop");
    fs::write(
        &checker,
        format!(
            "#!/bin/sh\nprintf 'stdout before wait\\n'\nprintf 'stderr before wait\\n' >&2\nprintf '%s\\n' \"$$\" > '{}'\nread release < '{}'\n",
            pid_file.display(),
            fifo.display(),
        ),
    )?;
    fs::set_permissions(&checker, fs::Permissions::from_mode(0o700))?;
    let tools = CaptureTools {
        vhs: PathBuf::new(),
        ttyd: PathBuf::new(),
        image_verifier: checker,
        dynamic_library_path: String::new(),
        image_timeout: Duration::from_secs(2),
        stop_file: cooperative_stop.then(|| stop_file.clone()),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker_root = root.clone();
    let worker = std::thread::spawn(move || {
        let result = tools
            .verify_image(
                &worker_root,
                "controlled-hang",
                &worker_root.join("unused.png"),
                false,
            )
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    let mut ready = true;
    let stop_request = if cooperative_stop {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !pid_file.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        ready = pid_file.exists();
        if ready {
            fs::write(&stop_file, "stop\n")
        } else {
            Ok(())
        }
    } else {
        Ok(())
    };
    let early = receiver.recv_timeout(Duration::from_secs(3));
    let was_bounded = early.is_ok();
    // The pre-fix checker is released by a protocol handshake, so RED cannot
    // leave a blocked process or thread behind. No unrelated process is signalled.
    let result = match early {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let mut release = fs::OpenOptions::new().write(true).open(&fifo)?;
            release.write_all(b"release\n")?;
            receiver.recv_timeout(Duration::from_secs(2))?
        }
        Err(error) => return Err(error.into()),
    };
    worker.join().map_err(|_| "checker worker panicked")?;
    stop_request?;
    ensure(
        ready,
        "controlled checker must reach readiness before cooperative stop",
    )?;
    let pid = fs::read_to_string(&pid_file).map_err(|error| {
        format!(
            "controlled checker did not publish {}: {error}; result={result:?}",
            pid_file.display()
        )
    })?;
    ensure(
        !Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .output()?
            .status
            .success(),
        "owned checker must be reaped before returning",
    )?;
    ensure(
        was_bounded,
        "image checker ignored its deadline until watchdog release",
    )?;
    ensure(
        result.is_err(),
        "hung checker cannot pass image verification",
    )?;
    if cooperative_stop {
        ensure(
            result
                .as_ref()
                .is_err_and(|error| error.contains("stop requested")),
            "active image child must observe private stop, not its later image timeout",
        )?;
    }
    ensure(
        fs::read_to_string(root.join("controlled-hang.image-observation.json"))?
            == "stdout before wait\n",
        "timeout must preserve stdout",
    )?;
    ensure(
        fs::read_to_string(root.join("controlled-hang.image-observation.stderr"))?
            == "stderr before wait\n",
        "timeout must preserve stderr",
    )?;
    fixture.assert_read_only()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn image_checker_success_nonzero_and_malformed_negative_preserve_outputs() -> TestResult<()> {
    let fixture = QuotaResetFixture::create().await?;
    for (label, stdout, exit_code, negative, accepted) in [
        (
            "success",
            "{\"checkpoint\":\"compact-quota-all\",\"recognizedLines\":[\"Quota status\"],\"missingMarkers\":[],\"unexpectedMarkers\":[]}",
            0,
            false,
            true,
        ),
        ("nonzero", "checker failed", 7, false, false),
        (
            "malformed-negative",
            "not a JSON scene rejection",
            1,
            true,
            false,
        ),
    ] {
        let output = fixture.root().join(label);
        fs::create_dir(&output)?;
        let checker = output.join("controlled-image-checker");
        fs::write(
            &checker,
            format!(
                "#!/bin/sh\nprintf '%s' '{stdout}'\nprintf 'controlled stderr' >&2\nexit {exit_code}\n"
            ),
        )?;
        fs::set_permissions(&checker, fs::Permissions::from_mode(0o700))?;
        let tools = CaptureTools {
            vhs: PathBuf::new(),
            ttyd: PathBuf::new(),
            image_verifier: checker,
            dynamic_library_path: String::new(),
            // This case proves output and exit semantics using the real checker
            // budget. The separate hung/stop cases above prove short deadlines.
            image_timeout: Duration::from_secs(30),
            stop_file: None,
        };
        let started = Instant::now();
        let observation = tools.verify_image(
            &output,
            "compact-quota-all",
            &output.join("unused.png"),
            negative,
        );
        ensure(
            observation.is_ok() == accepted,
            "controlled checker outcome must retain image success/rejection semantics",
        )?;
        let prefix = if negative {
            "reject-compact-quota-all-unused"
        } else {
            "compact-quota-all"
        };
        let actual_stdout =
            fs::read_to_string(output.join(format!("{prefix}.image-observation.json")))?;
        if actual_stdout != stdout {
            return Err(format!(
                "{label}: image stdout must survive success and failure; elapsed={:?}, observation={observation:?}, stdout={actual_stdout:?}, stderr={:?}",
                started.elapsed(),
                fs::read_to_string(output.join(format!("{prefix}.image-observation.stderr")))?,
            ).into());
        }
        ensure(
            fs::read_to_string(output.join(format!("{prefix}.image-observation.stderr")))?
                == "controlled stderr",
            "image stderr must survive success and failure",
        )?;
    }
    fixture.assert_read_only()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn private_stop_settles_owned_capture_without_waiting_for_its_vhs_deadline() -> TestResult<()>
{
    let fixture = QuotaResetFixture::create().await?;
    let output = fixture.root().join("controlled-static");
    fs::create_dir(&output)?;
    let fifo = fixture.root().join("controlled-release");
    ensure(
        Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()?
            .success(),
        "FIFO must exist",
    )?;
    let vhs = fixture.root().join("controlled-vhs");
    fs::write(
        &vhs,
        format!("#!/bin/sh\nread release < '{}'\n", fifo.display()),
    )?;
    fs::set_permissions(&vhs, fs::Permissions::from_mode(0o700))?;
    let stop_file = fixture.root().join("capture-stop");
    let tools = CaptureTools {
        vhs,
        ttyd: PathBuf::new(),
        image_verifier: PathBuf::new(),
        dynamic_library_path: String::new(),
        image_timeout: Duration::from_secs(30),
        stop_file: Some(stop_file.clone()),
    };
    let mut capture = tools.start_capture(&output, fixture.capability())?;
    let process_id = capture.child.id();
    fs::write(&stop_file, "stop\n")?;
    let stopped = capture.wait(Duration::from_secs(1));
    drop(capture);
    ensure(
        !Command::new("/bin/kill")
            .args(["-0", &process_id.to_string()])
            .output()?
            .status
            .success(),
        "cooperative stop must settle the owned process",
    )?;
    let error = stopped
        .expect_err("a private stop cannot become success")
        .to_string();
    ensure(
        error.contains("stop requested"),
        "stop must be observed before the VHS timeout",
    )?;
    let directory = fs::read_to_string(output.join("browser-temp.directory"))?;
    ensure(
        !std::path::Path::new(directory.trim()).exists(),
        "owned profile must be removed",
    )?;
    fixture.assert_read_only()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn private_stop_prevents_the_next_capture_or_image_child() -> TestResult<()> {
    let fixture = QuotaResetFixture::create().await?;
    let stop_file = fixture.root().join("capture-stop");
    let tools = CaptureTools {
        vhs: fixture.root().join("must-not-start"),
        ttyd: PathBuf::new(),
        image_verifier: fixture.root().join("must-not-start"),
        dynamic_library_path: String::new(),
        image_timeout: Duration::from_secs(30),
        stop_file: Some(stop_file.clone()),
    };
    let output = fixture.root().join("must-not-create");
    for bytes in [b"stop\n".as_slice(), b"invalid-stop".as_slice()] {
        fs::write(&stop_file, bytes)?;
        let error = match tools.start_capture(&output, fixture.capability()) {
            Ok(_) => return Err("stopped capture started a tool".into()),
            Err(error) => error.to_string(),
        };
        ensure(
            error.contains("stop"),
            "must reject the marker before tool startup",
        )?;
        ensure(
            tools
                .verify_image(
                    &output,
                    "compact-quota-all",
                    &output.join("unused.png"),
                    false,
                )
                .is_err(),
            "stopped image stage cannot start a checker",
        )?;
        ensure(
            !output.exists(),
            "stopped stage must not create output or a browser profile",
        )?;
    }
    fixture.assert_read_only()?;
    Ok(())
}

pub(super) async fn run_interrupted_capture_cleanup() -> TestResult<()> {
    let tools = CaptureTools::from_environment()?;
    let output_root = PathBuf::from(
        std::env::var_os("QUOTA_STORYBOARD_OUTPUT").ok_or("capture output directory missing")?,
    );
    fs::create_dir_all(&output_root)?;
    let output = output_root.join("interrupted-capture");
    fs::create_dir(&output)?;
    let output = fs::canonicalize(output)?;
    let fixture = QuotaResetFixture::create().await?;
    let provider = HeldLoopbackProvider::bind()?;
    let binary = fs::canonicalize(env!("CARGO_BIN_EXE_codex-router-quota-reset-test-harness"))?;
    tools.write_fixed_child_adapter(
        &output,
        &binary,
        &fixture,
        provider.address(),
        super::CapturedTuiSurface::Quota,
    )?;
    fs::write(
        output.join("quota.tape"),
        "Set Shell bash\nSet Columns 120\nSet Rows 36\nWait+Screen /ctrl-r account options/\nWait+Screen@30s /INTERRUPTED_CAPTURE_NEVER_MATCHES/\n",
    )?;
    let capture = tools.start_capture(&output, fixture.capability())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !output.join("quota-child.pid").exists() {
        ensure(
            Instant::now() < deadline,
            "interruption fixture child did not start",
        )?;
        std::thread::sleep(Duration::from_millis(25));
    }
    ensure(
        super::owned_capture::has_owned_browser(&output)?,
        "interruption must exercise a live owned headless browser",
    )?;
    // Exercise the guard's early-return/drop path while the real child is alive,
    // not just a successfully exited or already-cancelled VHS process.
    drop(capture);
    ensure_owned_child_stopped(&output)?;
    ensure(
        provider.finish()?.is_empty(),
        "interruption must make no provider requests",
    )?;
    fixture.assert_read_only()?;
    let fixture_root = fixture.root().to_path_buf();
    drop(fixture);
    ensure(
        !fixture_root.exists(),
        "interrupted fixture directory survived cleanup",
    )?;
    fs::write(
        output.join("cleanup-verified.txt"),
        "actual child and headless browser started; early drop reaped owned quota/wrapper/ttyd/browser; no provider requests; fixture read-only and removed\n",
    )?;
    Ok(())
}
