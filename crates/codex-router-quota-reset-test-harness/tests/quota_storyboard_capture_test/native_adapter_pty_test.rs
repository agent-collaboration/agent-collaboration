//! Native adapter protocol proof. Controlled stand-ins do not claim product appearance.
use super::TestResult;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::time::{Duration, Instant};
#[path = "native_adapter_fixture_support.rs"]
mod fixture_support;
use fixture_support::{
    NativeAdapter, NativeFixture, TIMEOUT, observe_process, process_group, require_condition,
    require_equal, require_process_absent, run_command,
};
const COMPONENT_ARGUMENTS: &str = "quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture\n--exact\n--ignored\n--nocapture\n--test-threads=1";

#[test]
fn component_adapter_selects_exact_fixture_and_requires_its_receipt() -> TestResult<()> {
    for mode in [
        "valid",
        "missing-receipt",
        "nonzero-child",
        "forged-child-marker",
    ] {
        let fixture = NativeFixture::prepare(mode)?;
        let adapter = NativeAdapter::spawn(&fixture, None)?;
        let (exit, transcript) = adapter.finish()?;
        require_equal(
            fixture.read("arguments.txt")?,
            COMPONENT_ARGUMENTS,
            "native assertion",
        )?;
        require_equal(fixture.read("scenario.txt")?, "normal", "native assertion")?;
        require_equal(
            fixture.read("component-process.exit")?,
            if mode == "nonzero-child" { "7" } else { "0" },
            "native assertion",
        )?;
        if mode == "valid" {
            require_equal(exit, 0, &transcript)?;
            require_equal(fixture.read("quota-child.exit")?, "0", "native assertion")?;
        } else {
            require_condition((exit) != (0), &format!("{mode}: {transcript}"))?;
            require_condition(
                (fixture.read("quota-child.exit")?) != ("0"),
                "native assertion",
            )?;
        }
        if mode == "forged-child-marker" {
            require_condition(
                transcript.contains("QUOTA_STORYBOARD_CHILD_EXIT=0"),
                "native assertion",
            )?;
            require_equal(fixture.read("quota-child.exit")?, "2", "native assertion")?;
        }
        require_process_absent(&fixture.read("quota-child.pid")?)?;
        fixture.cleanup()?;
    }
    Ok(())
}

#[test]
fn component_exit_marker_stays_on_the_final_cursor_line() -> TestResult<()> {
    let fixture = NativeFixture::prepare("valid")?;
    let (exit, transcript) = NativeAdapter::spawn(&fixture, None)?.finish()?;
    require_equal(exit, 0, &transcript)?;
    require_condition(transcript.matches('\n').count() > 12, "native assertion")?;
    require_equal(
        transcript.split('\n').next_back(),
        Some("QUOTA_STORYBOARD_CHILD_EXIT=0"),
        "native assertion",
    )?;
    require_equal(
        fixture.read("quota-child.viewport")?,
        "48 12",
        "native assertion",
    )?;
    fixture.cleanup()
}

#[test]
fn component_pending_receipt_requires_exact_release_and_fresh_paths() -> TestResult<()> {
    for mode in [
        "pending-valid",
        "pending-missing",
        "pending-wrong-release",
        "pending-wrong-receipt",
        "stale-receipt",
        "stale-release",
        "same-path",
    ] {
        let fixture = NativeFixture::prepare(mode)?;
        let mut adapter = NativeAdapter::spawn(&fixture, None)?;
        if mode == "pending-valid" {
            adapter.wait_ready()?;
            fs::write(fixture.output("fixture-release"), "settle\n")?;
        }
        let (exit, transcript) = adapter.finish()?;
        if mode == "pending-valid" {
            require_equal(exit, 0, &transcript)?;
        } else {
            require_condition((exit) != (0), &format!("{mode}: {transcript}"))?;
        }
        if mode.starts_with("stale-") || mode == "same-path" {
            require_condition(
                !fixture.output("arguments.txt").exists(),
                "native assertion",
            )?;
        }
        fixture.cleanup()?;
    }
    Ok(())
}

#[test]
fn adapter_keeps_existing_surfaces_and_rejects_unknown_scenario() -> TestResult<()> {
    for (mode, arguments) in [
        (
            "quota",
            "--router-root\n/owned/quoted ' root\n--fixture-capability\nowned-capability\n--provider-listener\n127.0.0.1:1234",
        ),
        (
            "sessions",
            "--sessions-picker\n--provider-filter-storyboard\n--fixture-capability\nowned-capability",
        ),
    ] {
        let fixture = NativeFixture::prepare(mode)?;
        let (exit, transcript) = NativeAdapter::spawn(&fixture, None)?.finish()?;
        require_equal(exit, 0, &transcript)?;
        require_equal(
            fixture.read("arguments.txt")?,
            arguments,
            "native assertion",
        )?;
        require_equal(
            fixture.read("home.txt")?,
            fixture.root.join("home").to_string_lossy(),
            "native assertion",
        )?;
        // Node/macOS may add its own runtime key; independently launch with exactly four keys.
        run_command(
            &fixture.root,
            "environment-baseline",
            Command::new(&fixture.node)
                .env_clear()
                .env("HOME", fixture.root.join("home"))
                .env("LANG", "C.UTF-8")
                .env("TERM", "xterm-256color")
                .env("TMPDIR", fixture.root.join("home"))
                .args(["-p", "Object.keys(process.env).sort().join('\\n')"]),
        )?;
        require_equal(
            fixture.read("environment-keys.txt")?,
            fs::read_to_string(fixture.root.join("environment-baseline.stdout"))?.trim_end(),
            "native assertion",
        )?;
        fixture.cleanup()?;
    }
    for mode in ["unknown-scenario", "unknown-surface"] {
        let fixture = NativeFixture::prepare(mode)?;
        let (exit, _) = NativeAdapter::spawn(&fixture, None)?.finish()?;
        require_condition((exit) != (0), "native assertion")?;
        require_condition(
            !fixture.output("arguments.txt").exists(),
            "native assertion",
        )?;
        fixture.cleanup()?;
    }
    Ok(())
}

#[test]
fn exact_public_argv_cwd_environment_and_host_guard() -> TestResult<()> {
    for codex_home in [None, Some("normal-codex-home")] {
        let fixture = NativeFixture::prepare("public")?;
        let (exit, transcript) = NativeAdapter::spawn(&fixture, codex_home)?.finish()?;
        require_equal(exit, 0, &transcript)?;
        require_equal(fixture.read("arguments.txt")?, "", "native assertion")?;
        require_equal(
            fixture.read("cwd.txt")?,
            fixture.root.join("approved source").to_string_lossy(),
            "native assertion",
        )?;
        require_equal(fixture.read("home.txt")?, "normal-home", "native assertion")?;
        require_equal(
            fixture.read("codex-home.txt")?,
            codex_home.unwrap_or("absent"),
            "native assertion",
        )?;
        require_equal(
            fixture.read("sentinel.txt")?,
            "preserved",
            "native assertion",
        )?;
        require_equal(
            fixture.read("router-root.txt")?,
            fixture.root.join("debug root").to_string_lossy(),
            "native assertion",
        )?;
        require_equal(
            fixture.read("quota-child.viewport")?,
            "48 12",
            "native assertion",
        )?;
        require_process_absent(&fixture.read("quota-child.pid")?)?;
        fixture.cleanup()?;
    }
    for mode in [
        "public-wrong-host",
        "public-wrong-binary",
        "public-extra-argument",
    ] {
        let fixture = NativeFixture::prepare(mode)?;
        let (exit, _) = NativeAdapter::spawn(&fixture, None)?.finish()?;
        require_condition((exit) != (0), "native assertion")?;
        require_condition(
            !fixture.output("arguments.txt").exists(),
            "native assertion",
        )?;
        fixture.cleanup()?;
    }
    Ok(())
}

#[test]
fn controlled_bystander_survives_terminal_hangup_but_not_termination() -> TestResult<()> {
    let fixture = NativeFixture::prepare("public-held-sibling")?;
    let mut adapter = NativeAdapter::spawn(&fixture, None)?;
    adapter.wait_ready()?;
    let deadline = Instant::now() + TIMEOUT;
    let bystander_pid = loop {
        if let Ok(pid) = fixture.read("bystander.pid")
            && pid.parse::<u32>().is_ok()
        {
            break pid;
        }
        if Instant::now() >= deadline {
            return Err("bystander PID readiness deadline".into());
        }
        adapter.receive(Duration::from_millis(20))?;
    };
    let before = observe_process(&bystander_pid)?;
    run_command(
        &fixture.root,
        "bystander-hangup",
        Command::new("/bin/kill").args(["-HUP", &bystander_pid]),
    )?;
    let deadline = Instant::now() + TIMEOUT;
    while fixture.read("bystander-hangup").ok().as_deref() != Some("SIGHUP\n") {
        if observe_process(&bystander_pid).is_err() {
            return Err("controlled bystander exited on terminal hangup".into());
        }
        if Instant::now() >= deadline {
            return Err("bystander hangup observation deadline".into());
        }
        adapter.receive(Duration::from_millis(20))?;
    }
    require_equal(
        observe_process(&bystander_pid)?,
        before,
        "hangup must preserve the controlled bystander identity",
    )?;
    run_command(
        &fixture.root,
        "bystander-terminate",
        Command::new("/bin/kill").args(["-TERM", &bystander_pid]),
    )?;
    require_process_absent(&bystander_pid)?;
    fs::write(fixture.root.join("release-child"), "release\n")?;
    let (exit, transcript) = adapter.finish()?;
    require_equal(exit, 0, &transcript)?;
    fixture.cleanup()
}

#[test]
fn wrapper_reaps_only_its_retained_public_child() -> TestResult<()> {
    let fixture = NativeFixture::prepare("public-held-sibling")?;
    let other = NativeFixture::prepare("public-held")?;
    let mut adapter = NativeAdapter::spawn(&fixture, None)?;
    let mut bystander = NativeAdapter::spawn(&other, None)?;
    adapter.wait_ready()?;
    bystander.wait_ready()?;
    let retained_pid = adapter.child.process_id().ok_or("wrapper PID missing")?;
    require_equal(
        fixture.read("quota-wrapper.pid")?,
        retained_pid.to_string(),
        "native assertion",
    )?;
    let sibling_deadline = Instant::now() + TIMEOUT;
    let sibling_pid = loop {
        if let Ok(pid) = fixture.read("bystander.pid")
            && pid.parse::<u32>().is_ok()
        {
            break pid;
        }
        if Instant::now() >= sibling_deadline {
            return Err("same-group bystander readiness deadline".into());
        }
        adapter.receive(Duration::from_millis(20))?;
    };
    let sibling_before = observe_process(&sibling_pid)?;
    require_equal(
        process_group(&sibling_pid)?,
        process_group(&retained_pid.to_string())?,
        "native assertion",
    )?;
    let child_pid = fixture.read("quota-child.pid")?;
    let identity = fixture.read("standin-identity.json")?;
    require_condition(
        identity.contains(&format!("\"pid\":{child_pid}")),
        "native assertion",
    )?;
    require_condition(
        identity.contains(&format!("\"parent\":{retained_pid}")),
        "native assertion",
    )?;
    require_condition(
        identity.contains(&format!("\"uid\":{}", fs::metadata(&fixture.root)?.uid())),
        "native assertion",
    )?;
    adapter.terminate_wrapper()?;
    let (exit, transcript) = adapter.finish()?;
    require_condition((exit) != (0), &transcript)?;
    require_equal(fixture.read("quota-child.exit")?, "-15", "native assertion")?;
    require_process_absent(&child_pid)?;
    require_process_absent(&retained_pid.to_string())?;
    require_condition(
        bystander.child.try_wait()?.is_none(),
        "unrelated retained wrapper was signaled",
    )?;
    require_condition(
        !other.output("quota-child.exit").exists(),
        "native assertion",
    )?;
    require_equal(
        observe_process(&sibling_pid)?,
        sibling_before,
        "same-group bystander was signaled",
    )?;
    fs::write(fixture.root.join("release-bystander"), "release\n")?;
    require_process_absent(&sibling_pid)?;
    fs::write(other.root.join("release-child"), "release\n")?;
    require_equal(bystander.finish()?.0, 0, "native assertion")?;
    require_process_absent(&other.read("quota-child.pid")?)?;
    fixture.cleanup()?;
    other.cleanup()
}

#[test]
fn same_child_kernel_resize_and_restore_reach_real_pty_eof() -> TestResult<()> {
    let fixture = NativeFixture::prepare("pending-valid")?;
    let mut adapter = NativeAdapter::spawn(&fixture, None)?;
    adapter.wait_ready()?;
    let wrapper_pid = fixture.read("quota-wrapper.pid")?;
    let child_pid = fixture.read("quota-child.pid")?;
    let before = observe_process(&child_pid)?;
    adapter.resize(120, 36)?;
    adapter.resize(48, 12)?;
    require_equal(
        fixture.read("quota-wrapper.pid")?,
        wrapper_pid.as_str(),
        "native assertion",
    )?;
    require_equal(
        fixture.read("quota-child.pid")?,
        child_pid.as_str(),
        "native assertion",
    )?;
    require_equal(observe_process(&child_pid)?, before, "native assertion")?;
    fs::write(fixture.output("fixture-release"), "settle\n")?;
    let (exit, transcript) = adapter.finish()?;
    require_equal(exit, 0, &transcript)?;
    require_equal(
        fixture.read("quota-child.viewport")?,
        "48 12",
        "native assertion",
    )?;
    require_process_absent(&child_pid)?;
    require_process_absent(&wrapper_pid)?;
    fixture.cleanup()
}

/// macOS installs /bin/ps setuid root: a user-launched ps has effective UID 0 and the
/// launching account's real UID. Hosts without that split cannot exercise this case.
fn setuid_system_ps_available() -> TestResult<bool> {
    let metadata = fs::metadata("/bin/ps")?;
    Ok(metadata.uid() == 0 && metadata.mode() & 0o4000 != 0)
}

#[test]
fn process_fingerprint_names_the_real_owner_of_a_user_launched_setuid_utility() -> TestResult<()> {
    if !setuid_system_ps_available()? {
        eprintln!(
            "skipped: host /bin/ps is not setuid root, so no effective/real UID split exists"
        );
        return Ok(());
    }
    // Repeated comm columns exceed what the pipe holds, so the actual setuid ps stays blocked in
    // write until this test drains it; only backpressure matters, not other processes' arguments.
    let mut held = Command::new("/bin/ps")
        .arg("-axww")
        .args(std::iter::repeat_n(["-o", "comm="], 40).flatten())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut stdout = held
        .stdout
        .take()
        .ok_or("held setuid utility stdout absent")?;
    let mut first = [0_u8; 1];
    std::io::Read::read_exact(&mut stdout, &mut first)?;
    let observed = observe_process(&held.id().to_string());
    let controller = observe_process(&std::process::id().to_string());
    std::io::copy(&mut stdout, &mut std::io::sink())?;
    require_condition(held.wait()?.success(), "held setuid utility failed")?;
    let owner = |line: &str| line.split_whitespace().nth(1).map(str::to_owned);
    let (observed, controller) = (observed?, controller?);
    require_equal(
        owner(&observed),
        owner(&controller),
        "setuid utility fingerprint must carry its launching account's real UID",
    )
}
