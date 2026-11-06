//! Running Router executable version and bounded launch-path observations.

use std::path::PathBuf;
use std::time::Duration;

pub use collaboration_protocol::RouterExecutableRelation;
use tokio::sync::watch;

const INSTALLED_VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// Shared observer retained by the Host and refreshed on status or periodic checks.
pub struct RouterExecutableObserver {
    launch_path: Option<PathBuf>,
    running_version: String,
    startup_identity: Option<ExecutableFileIdentity>,
    #[cfg(test)]
    installed_version_override: Option<String>,
    relation_sender: watch::Sender<RouterExecutableRelation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExecutableFileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

impl RouterExecutableObserver {
    /// Captures only the startup file's stat identity; it does no file reads or hashing.
    pub fn capture(launch_path: Result<PathBuf, std::io::Error>, running_version: String) -> Self {
        let launch_path = launch_path.ok();
        let startup_identity = launch_path
            .as_deref()
            .and_then(|path| executable_file_identity(path).ok());
        let initial_relation = RouterExecutableRelation::Unknown {
            reason: "the Router launch path has not been observed yet".to_owned(),
        };
        let (relation_sender, _relation_receiver) = watch::channel(initial_relation);
        Self {
            launch_path,
            running_version,
            startup_identity,
            #[cfg(test)]
            installed_version_override: None,
            relation_sender,
        }
    }

    /// Compares the launch path's stat identity and probes its version only after drift.
    pub async fn observe(&mut self) -> RouterExecutableRelation {
        let Some(path) = self.launch_path.as_deref() else {
            return self.publish_relation(RouterExecutableRelation::Unknown {
                reason: "the Router launch path is unavailable".to_owned(),
            });
        };
        let current_identity = match executable_file_identity(path) {
            Ok(identity) => identity,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return self.publish_relation(RouterExecutableRelation::Drift {
                    running_version: self.running_version.clone(),
                    installed_version: None,
                });
            }
            Err(_error) => {
                return self.publish_relation(RouterExecutableRelation::Unknown {
                    reason: "the Router launch path could not be inspected".to_owned(),
                });
            }
        };
        if self.startup_identity == Some(current_identity) {
            return self.publish_relation(RouterExecutableRelation::Match);
        }

        #[cfg(test)]
        let installed_version = match &self.installed_version_override {
            Some(version) => Ok(version.clone()),
            None => installed_version(path).await,
        };
        #[cfg(not(test))]
        let installed_version = installed_version(path).await;
        let relation = match installed_version {
            Ok(installed_version) if installed_version == self.running_version => {
                self.startup_identity = Some(current_identity);
                RouterExecutableRelation::Match
            }
            Ok(installed_version) => RouterExecutableRelation::Drift {
                running_version: self.running_version.clone(),
                installed_version: Some(installed_version),
            },
            Err(()) => RouterExecutableRelation::Unknown {
                reason: "the changed Router launch path could not report its version".to_owned(),
            },
        };
        self.publish_relation(relation)
    }

    /// Returns the latest relation without performing filesystem work.
    #[must_use]
    pub fn relation(&self) -> RouterExecutableRelation {
        self.relation_sender.borrow().clone()
    }

    /// Subscribes to the latest relation and every subsequent Host observation.
    pub fn subscribe(&self) -> watch::Receiver<RouterExecutableRelation> {
        self.relation_sender.subscribe()
    }

    fn publish_relation(&self, relation: RouterExecutableRelation) -> RouterExecutableRelation {
        self.relation_sender.send_replace(relation.clone());
        relation
    }
}

fn executable_file_identity(path: &std::path::Path) -> std::io::Result<ExecutableFileIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::metadata(path)?;
    Ok(ExecutableFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.size(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
    })
}

async fn installed_version(path: &std::path::Path) -> Result<String, ()> {
    let mut command = tokio::process::Command::new(path);
    command.arg("--version").kill_on_drop(true);
    let output = tokio::time::timeout(INSTALLED_VERSION_TIMEOUT, command.output())
        .await
        .map_err(|_elapsed| ())?
        .map_err(|_error| ())?;
    if !output.status.success() {
        return Err(());
    }
    codex_native_integration::parse_executable_version(&output.stdout).map_err(|_error| ())
}

#[cfg(test)]
#[path = "router_executable_observation_test.rs"]
mod tests;
