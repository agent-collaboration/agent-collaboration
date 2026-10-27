//! Bound capture tools and permanent adapter launches.
use super::*;
use std::os::unix::fs::OpenOptionsExt;

pub(super) struct CaptureTools {
    pub(super) vhs: PathBuf,
    pub(super) ttyd: PathBuf,
    pub(super) image_verifier: PathBuf,
    pub(super) dynamic_library_path: String,
    pub(super) image_timeout: Duration,
    pub(super) stop_file: Option<PathBuf>,
}

impl CaptureTools {
    pub(super) fn from_environment() -> TestResult<Self> {
        let (node, _) = verified_node_executable()?;
        let version = Command::new(node).arg("--version").output()?;
        ensure(
            version.status.success() && version.stdout == b"v26.12.1\n",
            "capture requires pinned Node v26.12.1",
        )?;
        Ok(Self {
            vhs: required_executable("QUOTA_STORYBOARD_VHS")?,
            ttyd: required_executable("QUOTA_STORYBOARD_TTYD")?,
            image_verifier: required_executable("QUOTA_STORYBOARD_IMAGE_VERIFIER")?,
            dynamic_library_path: std::env::var("QUOTA_STORYBOARD_DYLD_LIBRARY_PATH")?,
            image_timeout: Duration::from_secs(30),
            stop_file: std::env::var_os("QUOTA_STORYBOARD_OUTPUT")
                .filter(|_| std::env::var_os("QUOTA_STORYBOARD_CUA_SURFACE").is_none())
                .map(|root| PathBuf::from(root).join("capture-stop")),
        })
    }

    pub(super) fn write_fixed_child_adapter(
        &self,
        output: &Path,
        binary: &Path,
        fixture: &QuotaResetFixture,
        provider_address: std::net::SocketAddr,
        surface: CapturedTuiSurface,
    ) -> TestResult<()> {
        let configuration = format!(
            "{{\"binary\":{},\"root\":{},\"capability\":{},\"provider\":{},\"output\":{},\"ttyd\":{},\"temporary_directory\":{},\"dynamic_library_path\":{},\"surface\":{}}}\n",
            encode_json_string(&binary.to_string_lossy()),
            encode_json_string(&fixture.root().to_string_lossy()),
            encode_json_string(fixture.capability()),
            encode_json_string(&provider_address.to_string()),
            encode_json_string(&output.to_string_lossy()),
            encode_json_string(&self.ttyd.to_string_lossy()),
            encode_json_string(&std::env::temp_dir().to_string_lossy()),
            encode_json_string(&self.dynamic_library_path),
            encode_json_string(surface.label()),
        );
        fs::write(output.join("fixture-command.json"), configuration)?;
        self.write_child_adapter_files(output)
    }

    pub(super) fn write_child_adapter_files(&self, output: &Path) -> TestResult<()> {
        // The extracted Homebrew library retains an installed plugin directory.
        // libwebsockets also searches cwd; supply the exact same verified plugin
        // there without installing or rewriting any system library.
        let library_directory = std::env::split_paths(&self.dynamic_library_path)
            .next()
            .ok_or("isolated library directory is missing")?;
        fs::copy(
            library_directory.join("libwebsockets-evlib_uv.dylib"),
            output.join("libwebsockets-evlib_uv.dylib"),
        )?;
        let shim = output.join("shim");
        fs::create_dir(&shim)?;
        let (node, _) = verified_node_executable()?;
        let adapter = permanent_adapter_source("ttyd-adapter.ts")?;
        permanent_adapter_source("fixture-child.ts")?;
        let stderr = output.join("ttyd.stderr");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&stderr)?;
        fs::write(
            shim.join("ttyd"),
            format!(
                "#!/bin/sh\nexec {} {} \"$@\" 2>{}\n",
                shell_path(&node)?,
                shell_path(&adapter)?,
                shell_path(&stderr)?,
            ),
        )?;
        fs::set_permissions(shim.join("ttyd"), fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    pub(super) fn start_capture(
        &self,
        output: &Path,
        capability: &str,
    ) -> TestResult<OwnedCapture> {
        self.start_capture_with_owner(
            output,
            CapturedChildOwnership::FixtureCapability(capability.to_owned()),
        )
    }

    pub(super) fn start_capture_with_owner(
        &self,
        output: &Path,
        child_ownership: CapturedChildOwnership,
    ) -> TestResult<OwnedCapture> {
        ensure_capture_running(self.stop_file.as_deref())?;
        let stdout = fs::File::create(output.join("vhs.stdout"))?;
        let stderr = fs::File::create(output.join("vhs.stderr"))?;
        let browser_temporary_directory = create_browser_temporary_directory(output)?;
        let child = Command::new(&self.vhs)
            .arg("quota.tape")
            .current_dir(output)
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:/opt/homebrew/bin:/usr/bin:/bin",
                    output.join("shim").display()
                ),
            )
            .env("HOME", output)
            .env("TMPDIR", &browser_temporary_directory)
            .env("LANG", "C.UTF-8")
            .env("TERM", "xterm-256color")
            .env("DYLD_LIBRARY_PATH", &self.dynamic_library_path)
            .env(
                "QUOTA_STORYBOARD_FIXTURE_COMMAND",
                output.join("fixture-command.json"),
            )
            .stdout(stdout)
            .stderr(stderr)
            .process_group(0)
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                // No child exists yet to own this directory's cleanup.
                fs::remove_dir_all(&browser_temporary_directory)?;
                return Err(error.into());
            }
        };
        Ok(OwnedCapture {
            child,
            group_released: false,
            output: output.to_path_buf(),
            child_ownership,
            browser_temporary_directory,
            stop_file: self.stop_file.clone(),
        })
    }

    pub(super) fn verify_image(
        &self,
        output: &Path,
        checkpoint: &str,
        image_path: &Path,
        expect_rejection: bool,
    ) -> TestResult<()> {
        ensure_capture_running(self.stop_file.as_deref())?;
        let label = if expect_rejection {
            format!(
                "reject-{checkpoint}-{}",
                image_path
                    .file_stem()
                    .ok_or("image name missing")?
                    .to_string_lossy()
            )
        } else {
            checkpoint.to_owned()
        };
        let stdout_path = output.join(format!("{label}.image-observation.json"));
        let mut child = Command::new(&self.image_verifier)
            .arg(checkpoint)
            .arg(image_path)
            .stdin(std::process::Stdio::null())
            .stdout(fs::File::create(&stdout_path)?)
            .stderr(fs::File::create(
                output.join(format!("{label}.image-observation.stderr")),
            )?)
            .spawn()?;
        let record = fs::write(
            output.join(format!("{label}.image-verifier.pid")),
            child.id().to_string(),
        );
        if record.is_err() {
            let termination = child.kill();
            let settlement = child.wait();
            termination?;
            settlement?;
        }
        record?;
        let deadline = std::time::Instant::now() + self.image_timeout;
        let status = loop {
            if let Err(stopped) = ensure_capture_running(self.stop_file.as_deref()) {
                let termination = child.kill();
                let settlement = child.wait();
                termination?;
                settlement?;
                return Err(stopped);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => {
                    let termination = child.kill();
                    let settlement = child.wait();
                    termination?;
                    settlement?;
                    return match result {
                        Err(error) => Err(error.into()),
                        _ => Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "owned image checker exceeded its deadline; stdout/stderr retained",
                        )
                        .into()),
                    };
                }
            }
        };
        // Keep the checker's actual termination: complete output with no
        // markers and a nonzero status is a process failure, not a verdict.
        let termination = image_checker_termination(status);
        fs::write(
            output.join(format!("{label}.image-verifier.exit")),
            format!("{termination}\n"),
        )?;
        let stdout = fs::read(&stdout_path)?;
        if expect_rejection {
            if status.code() == Some(1)
                && String::from_utf8_lossy(&stdout).contains("missingMarkers")
            {
                Ok(())
            } else {
                Err(format!(
                    "blank or wrong-scene image must fail its scene oracle, not crash or fail to launch; checker {termination}"
                )
                .into())
            }
        } else if status.success() {
            Ok(())
        } else {
            Err(format!(
                "saved PNG failed its independent visible-scene oracle; checker {termination}"
            )
            .into())
        }
    }
}

/// The image checker's exit as JSON: its exit code, or the signal that ended it.
fn image_checker_termination(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    let json_number =
        |value: Option<i32>| value.map_or_else(|| "null".to_owned(), |number| number.to_string());
    format!(
        "{{\"code\":{},\"signal\":{}}}",
        json_number(status.code()),
        json_number(status.signal())
    )
}

pub(super) fn verified_node_executable() -> TestResult<(PathBuf, String)> {
    let path = required_executable("QUOTA_STORYBOARD_NODE")?;
    ensure(
        std::env::var_os("QUOTA_STORYBOARD_NODE").ok_or("Node nomination missing")? == path,
        "declared Node executable must retain its canonical absolute path",
    )?;
    let expected = std::env::var("QUOTA_STORYBOARD_NODE_SHA256")?;
    ensure(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && digest_file(&path)? == expected,
        "declared Node executable SHA256 changed",
    )?;
    Ok((path, expected))
}

pub(super) fn permanent_adapter_source(name: &str) -> TestResult<PathBuf> {
    let compiled: &[u8] = match name {
        "fixture-child.ts" => {
            include_bytes!("../../../../scripts/verification/tui/fixture-child.ts")
        }
        "ttyd-adapter.ts" => include_bytes!("../../../../scripts/verification/tui/ttyd-adapter.ts"),
        _ => return Err("unknown permanent TypeScript adapter".into()),
    };
    let path = fs::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/verification/tui")
            .join(name),
    )?;
    ensure(
        digest_file(&path)? == format!("{:x}", Sha256::digest(compiled)),
        "permanent adapter differs from compiled source binding",
    )?;
    Ok(path)
}

fn shell_path(path: &Path) -> TestResult<String> {
    Ok(format!(
        "'{}'",
        path.to_str()
            .ok_or("shell path must be UTF-8")?
            .replace('\'', "'\\''")
    ))
}

pub(super) fn required_executable(name: &str) -> TestResult<PathBuf> {
    let path = fs::canonicalize(std::env::var_os(name).ok_or_else(|| {
        std::io::Error::other(format!("{name} must nominate the exact isolated tool"))
    })?)?;
    ensure(path.is_file(), "capture tool must be an existing file")?;
    Ok(path)
}
