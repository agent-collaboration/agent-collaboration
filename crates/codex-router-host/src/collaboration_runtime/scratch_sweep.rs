//! Removes scratch folders unused for 30 days (R17).
//!
//! The sweep runs once when the Host starts its runtime and then on one timer,
//! set to the earliest moment any folder could become due: a folder's newest
//! change plus 30 days, or a retry one day after a failure. It never waits
//! longer than 29 days, so a scope a session holds open, now or from any
//! moment after this pass, has its use recorded again before 30 days pass.
//! The register of open scopes is memory only: after a crash and restart a
//! scope is judged by its last recorded use, which is never older than 29 days
//! for a scope that was open when the Host stopped. There is no fixed tick.
//!
//! For each folder: one with a session open in this Host is skipped and its
//! use recorded at the sweep's clock time; one whose state can't be read is kept and retried the next
//! day. A folder whose newest change is more than 30 days old is locked
//! exclusively without waiting (a creator holding it means skip), rechecked
//! under the lock, renamed to a hidden `.removing-` name, released and then
//! deleted. A failed removal is reported and retried the next day; the hidden
//! folder is deleted by a later pass.
//!
//! Claude Code and Cursor sessions don't report open scopes to the Host; their
//! folders are protected by the use stamp written at creation and by the
//! folder's newest change only.

use codex_native_integration::{
    prepare_scratch_scope_used_at, scratch_scope_last_change, try_lock_scratch_scope_exclusively,
};
use collaboration_runtime::OpenScratchScopes;
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime},
};

/// A scratch folder unused for longer than this is removed.
pub const SCRATCH_UNUSED_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// A folder that could not be judged or removed is tried again after this.
pub const SCRATCH_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest the sweep waits between passes: an open scope's use is recorded
/// again at least this often, a day inside the retention period.
pub const SCRATCH_OPEN_USE_INTERVAL: Duration =
    SCRATCH_UNUSED_RETENTION.saturating_sub(SCRATCH_RETRY_DELAY);
const REMOVING_PREFIX: &str = ".removing-";

/// The time source the sweep decides and waits by.
pub trait ScratchSweepClock: Send + Sync {
    fn now(&self) -> SystemTime;
    /// Completes once `now()` has reached `at`.
    fn sleep_until(&self, at: SystemTime) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// The wall clock, waiting on Tokio's timer.
pub struct SystemScratchSweepClock;

impl ScratchSweepClock for SystemScratchSweepClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep_until(&self, at: SystemTime) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let delay = at.duration_since(SystemTime::now()).unwrap_or_default();
        Box::pin(tokio::time::sleep(delay))
    }
}

/// What the sweep reports as it works.
pub trait ScratchSweepEvents: Send + Sync {
    /// The sweep holds `scope` exclusively and found it still unused; it will
    /// rename it away next. Runs on the sweep's blocking thread.
    fn rechecked(&self, _scope: &str) {}
    /// One pass finished.
    fn pass_finished(&self, report: &ScratchSweepReport);
}

/// Logs each pass's removals and failures.
pub struct LoggedScratchSweepEvents;

impl ScratchSweepEvents for LoggedScratchSweepEvents {
    fn pass_finished(&self, report: &ScratchSweepReport) {
        for scope in &report.removed {
            tracing::info!(scope = %scope, "removed scratch folder unused for 30 days");
        }
        for failure in &report.failures {
            tracing::warn!(
                scope = %failure.scope,
                error = %failure.error,
                "scratch folder kept; retried the next day"
            );
        }
    }
}

/// The outcome of one sweep pass.
#[derive(Debug, Default)]
pub struct ScratchSweepReport {
    /// The clock time the pass judged folders by.
    pub ran_at: Option<SystemTime>,
    pub removed: Vec<String>,
    /// Folders with a session open in this Host; their use was recorded.
    pub kept_open: Vec<String>,
    /// Folders used within 30 days.
    pub kept_recent: Vec<String>,
    /// Folders a creator or opener held when the sweep tried to lock them.
    pub kept_busy: Vec<String>,
    pub failures: Vec<ScratchSweepFailure>,
    /// When the next pass runs.
    pub next_pass_at: Option<SystemTime>,
}

#[derive(Debug)]
pub struct ScratchSweepFailure {
    pub scope: String,
    pub error: io::Error,
}

/// One pass over `scratch_root` at time `now`. Blocking filesystem work.
pub fn sweep_scratch_scopes(
    scratch_root: &Path,
    open: &OpenScratchScopes,
    now: SystemTime,
    events: &dyn ScratchSweepEvents,
) -> ScratchSweepReport {
    let mut pass = SweepPass {
        report: ScratchSweepReport {
            ran_at: Some(now),
            ..ScratchSweepReport::default()
        },
        next_pass_at: now + SCRATCH_OPEN_USE_INTERVAL,
        now,
    };
    let entries = match std::fs::read_dir(scratch_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return pass.finish(),
        Err(error) => {
            pass.fail(".", error);
            return pass.finish();
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                pass.fail(".", error);
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with(REMOVING_PREFIX) {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => pass.report.removed.push(name),
                Err(error) => pass.fail(&name, error),
            }
            continue;
        }
        let is_folder = std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir());
        if name.starts_with('.') || !is_folder {
            continue;
        }
        pass.judge(scratch_root, &name, &path, open, events);
    }
    pass.finish()
}

struct SweepPass {
    report: ScratchSweepReport,
    next_pass_at: SystemTime,
    now: SystemTime,
}

impl SweepPass {
    fn judge(
        &mut self,
        scratch_root: &Path,
        scope: &str,
        folder: &Path,
        open: &OpenScratchScopes,
        events: &dyn ScratchSweepEvents,
    ) {
        if open.is_open(folder) {
            self.keep_open(scope, folder);
            return;
        }
        let last_change = match scratch_scope_last_change(folder) {
            Ok(last_change) => last_change,
            Err(error) => return self.fail(scope, error),
        };
        if !self.unused(last_change) {
            self.keep_recent(scope, last_change);
            return;
        }
        let lock = match try_lock_scratch_scope_exclusively(scratch_root, scope) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.report.kept_busy.push(scope.to_owned());
                self.retry_next_day();
                return;
            }
            Err(error) => return self.fail(scope, error),
        };
        // Recheck under the lock: an opener or writer may have arrived since.
        if open.is_open(folder) {
            drop(lock);
            self.keep_open(scope, folder);
            return;
        }
        match scratch_scope_last_change(folder) {
            Ok(last_change) if !self.unused(last_change) => {
                self.keep_recent(scope, last_change);
                return;
            }
            Ok(_) => {}
            Err(error) => return self.fail(scope, error),
        }
        events.rechecked(scope);
        let hidden = match hidden_name(scratch_root, scope) {
            Ok(hidden) => hidden,
            Err(error) => return self.fail(scope, error),
        };
        let renamed = std::fs::rename(folder, &hidden);
        drop(lock);
        if let Err(error) = renamed {
            return self.fail(scope, error);
        }
        match std::fs::remove_dir_all(&hidden) {
            Ok(()) => self.report.removed.push(scope.to_owned()),
            Err(error) => self.fail(scope, error),
        }
    }

    fn unused(&self, last_change: SystemTime) -> bool {
        self.now
            .duration_since(last_change)
            .is_ok_and(|unused| unused > SCRATCH_UNUSED_RETENTION)
    }

    fn keep_open(&mut self, scope: &str, folder: &Path) {
        // Recorded at the time this pass judges by, so the next pass, at most
        // SCRATCH_OPEN_USE_INTERVAL later, records it again before it could be due.
        if let Err(error) = prepare_scratch_scope_used_at(folder, self.now) {
            self.fail(scope, error);
            return;
        }
        self.report.kept_open.push(scope.to_owned());
    }

    fn keep_recent(&mut self, scope: &str, last_change: SystemTime) {
        self.report.kept_recent.push(scope.to_owned());
        // A folder becomes removable just after 30 days without change.
        let due = (last_change + SCRATCH_UNUSED_RETENTION).max(self.now) + Duration::from_secs(1);
        self.next_pass_at = self.next_pass_at.min(due);
    }

    fn fail(&mut self, scope: &str, error: io::Error) {
        self.report.failures.push(ScratchSweepFailure {
            scope: scope.to_owned(),
            error,
        });
        self.retry_next_day();
    }

    fn retry_next_day(&mut self) {
        self.next_pass_at = self.next_pass_at.min(self.now + SCRATCH_RETRY_DELAY);
    }

    fn finish(mut self) -> ScratchSweepReport {
        self.report.next_pass_at = Some(self.next_pass_at);
        self.report
    }
}

fn hidden_name(scratch_root: &Path, scope: &str) -> io::Result<PathBuf> {
    let suffix = String::from(collaboration_runtime::new_service_uuid()?);
    Ok(scratch_root.join(format!("{REMOVING_PREFIX}{scope}-{suffix}")))
}

/// Runs a pass now, then one at each next due moment, until `shutdown`.
pub(super) async fn run_scratch_sweep(
    scratch_root: PathBuf,
    open: Arc<OpenScratchScopes>,
    clock: Arc<dyn ScratchSweepClock>,
    events: Arc<dyn ScratchSweepEvents>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    loop {
        let now = clock.now();
        let pass = {
            let scratch_root = scratch_root.clone();
            let open = Arc::clone(&open);
            let events = Arc::clone(&events);
            tokio::task::spawn_blocking(move || {
                let report = sweep_scratch_scopes(&scratch_root, &open, now, events.as_ref());
                events.pass_finished(&report);
                report.next_pass_at
            })
        };
        let next_pass_at = tokio::select! {
            () = shutdown.cancelled() => return,
            next = pass => match next {
                Ok(Some(next)) => next,
                // A pass that panicked or reported no deadline retries the next day.
                Ok(None) | Err(_) => now + SCRATCH_RETRY_DELAY,
            },
        };
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = clock.sleep_until(next_pass_at) => {}
        }
    }
}

#[cfg(test)]
#[path = "scratch_sweep_test.rs"]
mod tests;

impl super::CollaborationRuntime {
    /// Starts the scratch sweep for this Host's Router root. It runs a pass now
    /// and then at each next due moment by `clock`, until shutdown.
    pub fn start_scratch_sweep(
        &mut self,
        clock: Arc<dyn ScratchSweepClock>,
        events: Arc<dyn ScratchSweepEvents>,
    ) -> io::Result<()> {
        if self.scratch_sweep.is_some() {
            return Err(io::Error::other("the scratch sweep is already running"));
        }
        let router_root = self
            .directory
            .parent()
            .ok_or_else(|| io::Error::other("the collaboration directory has no Router root"))?;
        self.scratch_sweep = Some(tokio::spawn(run_scratch_sweep(
            codex_native_integration::router_scratch_root(router_root),
            Arc::clone(&self.open_scratch_scopes),
            clock,
            events,
            self.shutdown.clone(),
        )));
        Ok(())
    }
}
