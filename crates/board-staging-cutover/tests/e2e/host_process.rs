//! Retain the actual Host process and its native fixture until orderly stop is proved.
use super::{
    host_entrypoints::{OWNER_ENV, ROOT_ENV, ROUTER_ENV},
    host_operator,
    offline_cli_fixture::TestResult,
};
use router_peer_test_support::router_health::PersistentRouterHealthFixture;
use std::{
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

pub(super) struct OwnedHost {
    child: tokio::process::Child,
    root: PathBuf,
    router: PersistentRouterHealthFixture,
}

impl OwnedHost {
    pub async fn start(root: &Path, owner: &message_board::HumanId) -> TestResult<Self> {
        // The shared native fixture appends to an existing owned observation file.
        let _pid_log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(root.join("native-pids.log"))?;
        let router = PersistentRouterHealthFixture::start().await?;
        let output = std::fs::File::create(root.join("owned-host-output.log"))?;
        let child = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "host_entrypoints::owned_host_child",
                "--ignored",
                "--nocapture",
            ])
            .env(ROOT_ENV, root)
            .env(ROUTER_ENV, router.address().to_string())
            .env(OWNER_ENV, owner.as_str())
            .stdout(output.try_clone()?)
            .stderr(output)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        Ok(Self {
            child,
            root: root.to_owned(),
            router,
        })
    }
    pub async fn await_ready(&self) -> TestResult {
        let paths = codex_router_host::HostCoordinationPaths::for_router_root(&self.root);
        host_operator::await_ready(paths.operator_socket()).await
    }
    pub async fn stop(mut self) -> TestResult {
        let stopped = async {
            if self.child.try_wait()?.is_none() {
                let paths = codex_router_host::HostCoordinationPaths::for_router_root(&self.root);
                host_operator::stop(paths.operator_socket()).await?;
            }
            let status = tokio::time::timeout(Duration::from_secs(20), self.child.wait()).await??;
            if !status.success() {
                return Err("owned Host child did not stop successfully".into());
            }
            let output = std::fs::read_to_string(self.root.join("owned-host-output.log"))?;
            if !output.contains("1 passed; 0 failed") {
                return Err("owned Host test entrypoint did not finish".into());
            }
            let native_pids = std::fs::read_to_string(self.root.join("native-pids.log"))?;
            if native_pids.trim().is_empty() {
                return Err("owned native process inventory is unavailable".into());
            }
            for line in native_pids.lines() {
                let pid: u32 = line.parse()?;
                let absent = tokio::process::Command::new("/bin/ps")
                    .args(["-p", &pid.to_string(), "-o", "pid="])
                    .output()
                    .await
                    .map_err(|error| {
                        std::io::Error::new(
                            error.kind(),
                            format!("owned native absence probe: {error}"),
                        )
                    })?;
                if absent.status.code() != Some(1)
                    || !absent.stdout.is_empty()
                    || !absent.stderr.is_empty()
                {
                    return Err("owned native child still present or absence unknown".into());
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let router_stopped = self
            .router
            .finish()
            .await
            .map_err(|e| std::io::Error::other(e.to_string()));
        stopped?;
        router_stopped?;
        Ok(())
    }
}
