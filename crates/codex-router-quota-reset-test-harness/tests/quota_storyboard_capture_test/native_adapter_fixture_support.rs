//! Test-owned PTY, process and fixture support; no product appearance claim.
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::TestResult;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
pub(super) const TIMEOUT: Duration = Duration::from_secs(8);

pub(super) struct NativeFixture {
    pub(super) root: PathBuf,
    pub(super) node: PathBuf,
    wrapper: PathBuf,
}
impl NativeFixture {
    pub(super) fn prepare(mode: &str) -> TestResult<Self> {
        let namespace = module_path!()
            .rsplit("::")
            .nth(1)
            .ok_or("native fixture namespace missing")?;
        let root = std::env::temp_dir().join(format!(
            "2026-10-native-adapter-{namespace}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let node = match std::env::var_os("QUOTA_STORYBOARD_NODE") {
            Some(path) => PathBuf::from(path),
            None => std::env::split_paths(&std::env::var_os("PATH").ok_or("Node PATH missing")?)
                .map(|directory| directory.join("node"))
                .find(|path| path.is_file())
                .ok_or("Node executable missing")?,
        }
        .canonicalize()?;
        let package = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/verification/tui")
            .canonicalize()?;
        run_command(&root, "node-version", Command::new(&node).arg("--version"))?;
        if fs::read_to_string(root.join("node-version.stdout"))?.trim() != "v26.12.1" {
            return Err("native adapter tests require Node v26.12.1".into());
        }
        run_command(
            &root,
            "prepare",
            Command::new(&node)
                .arg(package.join("fixtures/native-adapter-fixture.ts"))
                .arg("prepare")
                .arg(&root)
                .arg(mode)
                .arg(&node),
        )?;
        Ok(Self {
            root,
            node,
            wrapper: package.join("fixtures/fixture-child.ts"),
        })
    }
    pub(super) fn output(&self, name: &str) -> PathBuf {
        self.root.join("output").join(name)
    }
    pub(super) fn read(&self, name: &str) -> TestResult<String> {
        Ok(fs::read_to_string(self.output(name))?)
    }
    pub(super) fn cleanup(self) -> TestResult<()> {
        fs::remove_dir_all(&self.root)?;
        Ok(())
    }
}

impl Drop for NativeFixture {
    fn drop(&mut self) {
        for name in ["release-child", "release-bystander"] {
            let _ = fs::write(self.root.join(name), "release\n");
        }
    }
}

pub(super) fn run_command(root: &Path, label: &str, command: &mut Command) -> TestResult<()> {
    let stdout = fs::File::create(root.join(format!("{label}.stdout")))?;
    let stderr = fs::File::create(root.join(format!("{label}.stderr")))?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()?;
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("{label} failed; retained {}", root.display()).into())
            };
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let reap_deadline = Instant::now() + Duration::from_secs(2);
            while child.try_wait()?.is_none() && Instant::now() < reap_deadline {
                thread::sleep(Duration::from_millis(5));
            }
            return Err(format!("{label} deadline; retained {}", root.display()).into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

enum TerminalRead {
    Bytes(Vec<u8>),
    Eof,
    Failure(String),
}
pub(super) struct NativeAdapter {
    root: PathBuf,
    master: Option<Box<dyn MasterPty + Send>>,
    pub(super) child: Box<dyn Child + Send + Sync>,
    receiver: Receiver<TerminalRead>,
    reader: Option<JoinHandle<()>>,
    transcript: Vec<u8>,
    eof: bool,
    settled: bool,
}
impl NativeAdapter {
    pub(super) fn spawn(fixture: &NativeFixture, codex_home: Option<&str>) -> TestResult<Self> {
        let pair = native_pty_system().openpty(size(48, 12))?;
        let mut command = CommandBuilder::new(&fixture.node);
        command.arg(&fixture.wrapper);
        command.arg(fixture.root.join("configuration.json"));
        command.env_clear();
        command.env(
            "QUOTA_STORYBOARD_FIXTURE_COMMAND",
            fixture.root.join("configuration.json"),
        );
        command.env("HOME", "normal-home");
        command.env("PUBLIC_TEST_SENTINEL", "preserved");
        if let Some(home) = codex_home {
            command.env("CODEX_HOME", home);
        }
        command.cwd(&fixture.root);
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(length) => {
                        let Some(bytes) = buffer.get(..length) else {
                            let _ = sender
                                .send(TerminalRead::Failure("PTY read exceeded buffer".to_owned()));
                            return;
                        };
                        if sender.send(TerminalRead::Bytes(bytes.to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(error) if error.raw_os_error() == Some(5) => break,
                    Err(error) => {
                        let _ = sender.send(TerminalRead::Failure(error.to_string()));
                        return;
                    }
                }
            }
            let _ = sender.send(TerminalRead::Eof);
        });
        Ok(Self {
            root: fixture.root.clone(),
            master: Some(pair.master),
            child,
            receiver,
            reader: Some(reader),
            transcript: Vec::new(),
            eof: false,
            settled: false,
        })
    }
    pub(super) fn receive(&mut self, duration: Duration) -> TestResult<()> {
        match self.receiver.recv_timeout(duration) {
            Ok(TerminalRead::Bytes(bytes)) => self.transcript.extend(bytes),
            Ok(TerminalRead::Eof) => self.eof = true,
            Ok(TerminalRead::Failure(error)) => return Err(error.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) if self.eof => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("PTY reader disconnected without EOF".into());
            }
        }
        Ok(())
    }
    pub(super) fn wait_ready(&mut self) -> TestResult<()> {
        let deadline = Instant::now() + TIMEOUT;
        while !String::from_utf8_lossy(&self.transcript).contains("NATIVE_ADAPTER_READY") {
            if self.child.try_wait()?.is_some() || Instant::now() >= deadline {
                return Err(format!(
                    "adapter readiness failed; retained {}: {}",
                    self.root.display(),
                    String::from_utf8_lossy(&self.transcript)
                )
                .into());
            }
            self.receive(Duration::from_millis(20))?;
        }
        Ok(())
    }
    pub(super) fn resize(&mut self, columns: u16, rows: u16) -> TestResult<()> {
        let master = self.master.as_ref().ok_or("missing PTY master")?;
        master.resize(size(columns, rows))?;
        if master.get_size()? != size(columns, rows) {
            return Err("kernel PTY size differs from requested viewport".into());
        }
        let expected = format!("{columns} {rows}");
        let deadline = Instant::now() + TIMEOUT;
        while fs::read_to_string(self.root.join("output/quota-child.viewport"))
            .ok()
            .as_deref()
            != Some(expected.as_str())
        {
            if Instant::now() >= deadline {
                return Err("kernel viewport readback deadline".into());
            }
            self.receive(Duration::from_millis(20))?;
        }
        Ok(())
    }
    pub(super) fn terminate_wrapper(&self) -> TestResult<()> {
        let pid = self
            .child
            .process_id()
            .ok_or("retained wrapper PID missing")?;
        run_command(
            &self.root,
            "signal",
            Command::new("/bin/kill").args(["-TERM", &pid.to_string()]),
        )
    }
    pub(super) fn finish(mut self) -> TestResult<(u32, String)> {
        let deadline = Instant::now() + TIMEOUT;
        let mut code = None;
        while code.is_none() || !self.eof {
            if let Some(status) = self.child.try_wait()? {
                code = Some(status.exit_code());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "PTY exit/EOF deadline; retained {}: {}",
                    self.root.display(),
                    String::from_utf8_lossy(&self.transcript)
                )
                .into());
            }
            self.receive(Duration::from_millis(20))?;
        }
        if let Some(pid) = self.child.process_id() {
            require_process_absent(&pid.to_string())?;
        }
        let child_pid_path = self.root.join("output/quota-child.pid");
        if child_pid_path.is_file() {
            require_process_absent(&fs::read_to_string(child_pid_path)?)?;
        }
        self.settled = true;
        if let Some(reader) = self.reader.take() {
            reader.join().map_err(|_| "PTY reader panic")?;
        }
        self.master.take();
        Ok((
            code.ok_or("missing adapter exit")?,
            String::from_utf8(std::mem::take(&mut self.transcript))?,
        ))
    }
}
impl Drop for NativeAdapter {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Only this stand-in's release and retained wrapper are touched on failure.
        let _ = fs::write(self.root.join("release-child"), "release\n");
        let _ = fs::write(self.root.join("release-bystander"), "release\n");
        if !self.root.join("output/fixture-release").exists() {
            let _ = fs::write(self.root.join("output/fixture-release"), "settle\n");
        }
        let _ = self.terminate_wrapper();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = fs::write(
            self.root.join("cleanup-unsettled"),
            "retained wrapper or inherited writer settlement requires inspection\n",
        );
    }
}
fn size(columns: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols: columns,
        pixel_width: 0,
        pixel_height: 0,
    }
}
pub(super) fn require_process_absent(pid: &str) -> TestResult<()> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let result = Command::new("/bin/ps")
            .args(["-p", pid.trim(), "-o", "pid="])
            .output()?;
        if result.status.code() == Some(1) && result.stdout.is_empty() && result.stderr.is_empty() {
            return Ok(());
        }
        if result.status.code() != Some(0) || Instant::now() >= deadline {
            return Err(format!("owned process remains or observation failed: {pid}").into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}
pub(super) fn process_group(pid: &str) -> TestResult<String> {
    let result = Command::new("/bin/ps")
        .args(["-p", pid.trim(), "-o", "pgid="])
        .output()?;
    if !result.status.success() {
        return Err("controlled process group unavailable".into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

/// A stable before/after fingerprint: PID, real owner UID (`ruid`, the launching account even for
/// a setuid utility), group, start time and command name. Effective UID is not part of it.
pub(super) fn observe_process(pid: &str) -> TestResult<String> {
    let observed = Command::new("/bin/ps")
        .args(["-p", pid.trim(), "-o", "pid=,ruid=,pgid=,lstart=,comm="])
        .output()?;
    if !observed.status.success() {
        return Err("controlled child identity unavailable".into());
    }
    Ok(String::from_utf8(observed.stdout)?)
}

pub(super) fn require_condition(condition: bool, context: &str) -> TestResult<()> {
    if !condition {
        return Err(context.to_owned().into());
    }
    Ok(())
}
pub(super) fn require_equal<
    Actual: std::fmt::Debug + PartialEq<Expected>,
    Expected: std::fmt::Debug,
>(
    actual: Actual,
    expected: Expected,
    context: &str,
) -> TestResult<()> {
    if actual != expected {
        return Err(format!("{context}: actual={actual:?}, expected={expected:?}").into());
    }
    Ok(())
}
