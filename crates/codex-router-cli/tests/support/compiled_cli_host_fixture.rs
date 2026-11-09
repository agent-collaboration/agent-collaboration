//! Signing and exact-root cleanup for the compiled Host installation fixture.
use std::io;
use std::path::Path;
use std::process::Command;

pub async fn sign_debug_binary(binary: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let signed =
        tokio::process::Command::new(repository.join("scripts/cargo_debug_signing_runner.sh"))
            .arg("--sign-only")
            .arg(binary)
            .kill_on_drop(true)
            .output()
            .await?;
    if !signed.status.success()
        || String::from_utf8_lossy(&signed.stderr).contains("stays linker-signed")
    {
        return Err(io::Error::other(format!(
            "copied fixture binary signing failed: {}",
            String::from_utf8_lossy(&signed.stderr)
        ))
        .into());
    }
    #[cfg(target_os = "macos")]
    {
        let signature = Command::new("codesign").arg("-dv").arg(binary).output()?;
        if !signature.status.success()
            || !String::from_utf8_lossy(&signature.stderr)
                .lines()
                .any(|line| line == "TeamIdentifier=974QD84WVC")
            || !Command::new("codesign")
                .arg("--verify")
                .arg(binary)
                .status()?
                .success()
        {
            return Err(io::Error::other(
                "copied fixture binary lacks a verified Developer ID signature",
            )
            .into());
        }
        eprintln!(
            "fixture_debug_signature binary={} team=974QD84WVC verified=true",
            binary.display()
        );
    }
    Ok(())
}

/// Signal exact-root leftovers on failure; their Child owners must join exit.
/// A true result means the directory must remain until those joins complete.
pub fn kill_fixture_processes(directory: &Path) -> io::Result<bool> {
    let canonical = directory.canonicalize()?;
    let generations =
        std::fs::read_to_string(directory.join("app-generations.log")).unwrap_or_default();
    let processes = Command::new("/bin/ps")
        .args(["-Ao", "pid=,command="])
        .output()?;
    if !processes.status.success() {
        return Err(io::Error::other("cannot inspect owned fixture processes"));
    }
    let mut signaled_owned_process = false;
    for line in String::from_utf8_lossy(&processes.stdout).lines() {
        let Some((process, command)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(raw_process) = process.parse::<i32>() else {
            continue;
        };
        let command = command.trim_start();
        let executable = command.split_whitespace().next().unwrap_or_default();
        let shell_script = matches!(
            Path::new(executable).file_name().and_then(|s| s.to_str()),
            Some("bash" | "sh")
        )
        .then(|| command.split_whitespace().nth(1))
        .flatten();
        let rooted = [Some(executable), shell_script]
            .into_iter()
            .flatten()
            .any(|path| {
                Path::new(path).starts_with(directory) || Path::new(path).starts_with(&canonical)
            });
        let recorded_child = generations.lines().any(|recorded| recorded == process)
            && command.contains("::compiled_cli_app_server_child_entrypoint");
        if !(rooted || recorded_child) {
            continue;
        }
        let Some(process) = rustix::process::Pid::from_raw(raw_process) else {
            return Err(io::Error::other("fixture process has an invalid PID"));
        };
        match rustix::process::kill_process(process, rustix::process::Signal::KILL) {
            Ok(()) => signaled_owned_process = true,
            Err(rustix::io::Errno::SRCH) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(signaled_owned_process)
}
