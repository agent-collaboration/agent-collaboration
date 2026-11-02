//! Own the restart fixture task and every child until exact-PID termination and reap.
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use codex_router_host::{HostError, HostExit};
use rustix::process::{Pid, Signal, WaitOptions};
use tokio::task::JoinHandle;

pub(super) static FIXTURE_SCOPE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub(super) const REGISTRY_ENVIRONMENT: &str = "CODEX_HOST_RESTART_CHILD_REGISTRY";

pub(super) struct RestartFixtureLifetime {
    registry: PathBuf,
    runtime: Option<JoinHandle<Result<HostExit, HostError>>>,
    finished: bool,
}

impl RestartFixtureLifetime {
    pub(super) fn new(registry: PathBuf) -> std::io::Result<Self> {
        std::fs::write(&registry, b"")?;
        Ok(Self {
            registry,
            runtime: None,
            finished: false,
        })
    }

    pub(super) fn attach(&mut self, runtime: JoinHandle<Result<HostExit, HostError>>) {
        self.runtime = Some(runtime);
    }

    pub(super) async fn finish(&mut self) -> std::io::Result<()> {
        if let Some(runtime) = self.runtime.take() {
            runtime.abort();
            let _cancelled = runtime.await;
        }
        self.stop_and_reap()
    }

    fn stop_and_reap(&mut self) -> std::io::Result<()> {
        if self.finished {
            return Ok(());
        }
        // Registration is serialized with closing admission. A child that starts
        // after this point exits before opening any fixture listener.
        let process_ids = close_registration(&self.registry)?;
        let mut failures = Vec::new();
        for process_id in process_ids {
            if let Err(error) = stop_and_reap_process(process_id) {
                failures.push(error.to_string());
            }
        }
        // This integration-test binary serializes its fixture scopes; its
        // other test entries spawn nothing. After abort/join no more children
        // can be launched. Drain even a child still booting the test harness,
        // which will see closed registration before it can serve a listener.
        loop {
            match rustix::process::waitpid(None, WaitOptions::empty()) {
                Ok(Some(_)) | Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::CHILD) => break,
                Ok(None) => failures.push("fixture reap returned no terminal status".to_owned()),
                Err(error) => {
                    failures.push(error.to_string());
                    break;
                }
            }
        }
        self.finished = true;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(std::io::Error::other(failures.join("; ")))
        }
    }
}

impl Drop for RestartFixtureLifetime {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.abort();
        }
        if let Err(error) = self.stop_and_reap() {
            eprintln!("restart fixture cleanup failed: {error}");
        }
    }
}

pub(super) fn register_fixture_child() -> std::io::Result<()> {
    let path = std::env::var_os(REGISTRY_ENVIRONMENT)
        .ok_or_else(|| std::io::Error::other("fixture child registry is missing"))?;
    let mut registry = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    registry.lock()?;
    let mut contents = String::new();
    registry.read_to_string(&mut contents)?;
    if contents.lines().any(|line| line == "closed") {
        return Err(std::io::Error::other("fixture lifetime already ended"));
    }
    writeln!(registry, "{}", std::process::id())?;
    registry.unlock()
}

fn close_registration(path: &Path) -> std::io::Result<Vec<u32>> {
    let mut registry = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    registry.lock()?;
    let mut contents = String::new();
    registry.read_to_string(&mut contents)?;
    registry.seek(std::io::SeekFrom::End(0))?;
    writeln!(registry, "closed")?;
    registry.unlock()?;
    contents
        .lines()
        .filter(|line| *line != "closed")
        .map(|line| line.parse().map_err(std::io::Error::other))
        .collect()
}

pub(super) fn stop_and_reap_process(process_id: u32) -> std::io::Result<()> {
    let process_id = Pid::from_raw(i32::try_from(process_id).map_err(std::io::Error::other)?)
        .ok_or_else(|| std::io::Error::other("fixture process ID must be nonzero"))?;
    // Do not signal an old PID already reaped by the Host: it could be reused.
    match rustix::process::waitpid(Some(process_id), WaitOptions::NOHANG) {
        Ok(Some(_)) | Err(rustix::io::Errno::CHILD) => return Ok(()),
        Ok(None) => {}
        Err(error) => return Err(error.into()),
    }
    match rustix::process::kill_process(process_id, Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => {}
        Err(error) => return Err(error.into()),
    }
    loop {
        match rustix::process::waitpid(Some(process_id), WaitOptions::empty()) {
            Ok(Some(_)) | Err(rustix::io::Errno::CHILD) => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Ok(None) => return Err(std::io::Error::other("fixture child was not reaped")),
            Err(error) => return Err(error.into()),
        }
    }
}
