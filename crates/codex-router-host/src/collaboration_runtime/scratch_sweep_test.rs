//! J10: the Host's registered scratch sweep, driven by an injected clock.
use super::*;
use crate::{CollaborationRuntime, CollaborationRuntimeInputs};
use codex_acp_adapter::ScratchScopeRegister as _;
use std::{
    os::unix::fs::PermissionsExt as _,
    sync::{Mutex, mpsc as std_mpsc},
};
use tokio::sync::{Notify, mpsc};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// One look a sleeping sweep took at the clock: the deadline it waits for, the
/// clock time it saw, and whether that look ended the wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimerCheck {
    deadline: SystemTime,
    seen_at: SystemTime,
    fired: bool,
}

/// A clock the test moves by hand; waiters wake when it moves, and every look
/// a waiter takes is reported, so the test knows what the timer decided.
struct ManualClock {
    now: Mutex<SystemTime>,
    moved: Notify,
    checks: mpsc::UnboundedSender<TimerCheck>,
    checks_seen: Mutex<Option<mpsc::UnboundedReceiver<TimerCheck>>>,
}

impl ManualClock {
    fn starting_now() -> Arc<Self> {
        let (checks, checks_seen) = mpsc::unbounded_channel();
        Arc::new(Self {
            now: Mutex::new(SystemTime::now()),
            moved: Notify::new(),
            checks,
            checks_seen: Mutex::new(Some(checks_seen)),
        })
    }

    fn timer_checks(&self) -> mpsc::UnboundedReceiver<TimerCheck> {
        self.checks_seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or_else(|| panic!("timer checks already taken"))
    }

    fn advance(&self, by: Duration) {
        *self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += by;
        self.moved.notify_waiters();
    }
}

impl ScratchSweepClock for ManualClock {
    fn now(&self) -> SystemTime {
        *self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn sleep_until(&self, at: SystemTime) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            loop {
                let moved = self.moved.notified();
                tokio::pin!(moved);
                moved.as_mut().enable();
                let seen_at = self.now();
                let fired = seen_at >= at;
                let _ = self.checks.send(TimerCheck {
                    deadline: at,
                    seen_at,
                    fired,
                });
                if fired {
                    return;
                }
                moved.await;
            }
        })
    }
}

/// The scope to hold after its recheck, a signal that it was reached, and the
/// release the sweep waits for.
type HeldRecheck = (String, std_mpsc::Sender<()>, std_mpsc::Receiver<()>);

/// Sends each finished pass to the test; can hold the sweep right after its
/// recheck of one scope until the test releases it.
struct ObservedEvents {
    passes: mpsc::UnboundedSender<ScratchSweepReport>,
    hold_after_recheck: Mutex<Option<HeldRecheck>>,
}

impl ScratchSweepEvents for ObservedEvents {
    fn rechecked(&self, scope: &str) {
        let held = self
            .hold_after_recheck
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_if(|(held, _, _)| held == scope);
        if let Some((_, reached, release)) = held {
            let _ = reached.send(());
            let _ = release.recv();
        }
    }

    fn pass_finished(&self, report: &ScratchSweepReport) {
        let _ = self.passes.send(ScratchSweepReport {
            ran_at: report.ran_at,
            removed: report.removed.clone(),
            kept_open: report.kept_open.clone(),
            kept_recent: report.kept_recent.clone(),
            kept_busy: report.kept_busy.clone(),
            failures: report
                .failures
                .iter()
                .map(|failure| ScratchSweepFailure {
                    scope: failure.scope.clone(),
                    error: io::Error::new(failure.error.kind(), failure.error.to_string()),
                })
                .collect(),
            next_pass_at: report.next_pass_at,
        });
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    scratch: PathBuf,
    runtime: CollaborationRuntime,
    clock: Arc<ManualClock>,
    passes: mpsc::UnboundedReceiver<ScratchSweepReport>,
    events: Arc<ObservedEvents>,
}

fn private(path: &Path) {
    std::fs::create_dir_all(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("private {}: {error}", path.display()));
}

async fn start_runtime(router_root: &Path) -> CollaborationRuntime {
    let directory = router_root.join("agent-communication");
    private(&directory);
    CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: directory.clone(),
        codex_home: directory.clone(),
        backend_socket: directory.join("backend.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: Some(directory.join("claude-sessions")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await
    .unwrap_or_else(|error| panic!("runtime: {error}"))
}

impl Fixture {
    /// A started Host runtime whose scratch folders are seeded by `seed`
    /// before the sweep's first pass.
    async fn start(seed: impl FnOnce(&Path, SystemTime)) -> Self {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("root: {error}"));
        private(root.path());
        let scratch = root.path().join("scratch");
        private(&scratch);
        let clock = ManualClock::starting_now();
        seed(&scratch, clock.now());
        let runtime = start_runtime(root.path()).await;
        let (sender, passes) = mpsc::unbounded_channel();
        let events = Arc::new(ObservedEvents {
            passes: sender,
            hold_after_recheck: Mutex::new(None),
        });
        Self {
            _root: root,
            scratch,
            runtime,
            clock,
            passes,
            events,
        }
    }

    fn start_sweep(&mut self) {
        self.runtime
            .start_scratch_sweep(
                Arc::clone(&self.clock) as Arc<dyn ScratchSweepClock>,
                Arc::clone(&self.events) as Arc<dyn ScratchSweepEvents>,
            )
            .unwrap_or_else(|error| panic!("sweep: {error}"));
    }

    async fn next_pass(&mut self) -> ScratchSweepReport {
        tokio::time::timeout(Duration::from_secs(10), self.passes.recv())
            .await
            .unwrap_or_else(|_| panic!("no sweep pass reported"))
            .unwrap_or_else(|| panic!("sweep stopped"))
    }

    fn folder(&self, scope: &str) -> PathBuf {
        self.scratch.join(scope)
    }

    async fn stop(self) {
        self.runtime
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown: {error}"));
    }
}

async fn next_check(checks: &mut mpsc::UnboundedReceiver<TimerCheck>) -> TimerCheck {
    tokio::time::timeout(Duration::from_secs(10), checks.recv())
        .await
        .unwrap_or_else(|_| panic!("the sweep timer took no look at the clock"))
        .unwrap_or_else(|| panic!("clock dropped"))
}

/// A scope folder whose every entry was last changed `age` before `now`.
fn seed_scope(scratch: &Path, scope: &str, now: SystemTime, age: Duration) {
    let folder = scratch.join(scope);
    private(&folder);
    let note = folder.join("notes.md");
    std::fs::write(&note, b"scratch").unwrap_or_else(|error| panic!("note: {error}"));
    set_changed(&note, now - age);
    set_changed(&folder, now - age);
}

fn set_changed(path: &Path, at: SystemTime) {
    std::fs::File::open(path)
        .and_then(|file| file.set_modified(at))
        .unwrap_or_else(|error| panic!("age {}: {error}", path.display()));
}

#[tokio::test]
async fn first_pass_removes_unused_folders_and_keeps_recent_open_and_recently_changed_ones() {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "unused-31", now, 31 * DAY);
        seed_scope(scratch, "unused-29", now, 29 * DAY);
        seed_scope(scratch, "open-31", now, 31 * DAY);
        seed_scope(scratch, "nested-change", now, 40 * DAY);
        let nested = scratch.join("nested-change/deep");
        private(&nested);
        std::fs::write(nested.join("fresh.txt"), b"x").unwrap_or_else(|error| panic!("{error}"));
        set_changed(&nested.join("fresh.txt"), now - 2 * DAY);
        set_changed(&nested, now - 40 * DAY);
        set_changed(&scratch.join("nested-change"), now - 40 * DAY);
    })
    .await;
    fixture
        .runtime
        .open_scratch_scopes
        .opened(&fixture.folder("open-31"));
    fixture.start_sweep();

    let pass = fixture.next_pass().await;

    assert_eq!(pass.removed, vec!["unused-31".to_owned()]);
    assert!(!fixture.folder("unused-31").exists());
    assert!(fixture.folder("unused-29").is_dir(), "29 days unused stays");
    assert!(
        fixture.folder("open-31").is_dir(),
        "an open session keeps its folder"
    );
    assert_eq!(pass.kept_open, vec!["open-31".to_owned()]);
    assert!(
        fixture
            .folder("open-31")
            .join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME)
            .is_file(),
        "a skipped open scope has its use recorded"
    );
    assert!(
        fixture.folder("nested-change").is_dir(),
        "a recent change inside keeps it"
    );
    assert!(pass.failures.is_empty(), "{:?}", pass.failures);
    fixture.stop().await;
}

#[tokio::test]
async fn a_shared_thread_folder_stays_while_any_session_is_open_and_is_removed_30_days_after_the_last_close()
 {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "thread-root", now, 40 * DAY);
    })
    .await;
    let folder = fixture.folder("thread-root");
    let open = Arc::clone(&fixture.runtime.open_scratch_scopes);
    open.opened(&folder);
    open.opened(&folder);
    open.closed(&folder);
    fixture.start_sweep();

    let first = fixture.next_pass().await;
    assert_eq!(first.kept_open, vec!["thread-root".to_owned()]);
    assert!(folder.is_dir());

    // The last session closes (recording a use); 31 quiet days later it goes.
    open.closed(&folder);
    fixture.clock.advance(31 * DAY);
    let second = fixture.next_pass().await;
    assert_eq!(second.removed, vec!["thread-root".to_owned()]);
    assert!(!folder.exists());
    fixture.stop().await;
}

#[tokio::test]
async fn no_pass_runs_between_due_moments() {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "due-in-two-days", now, 28 * DAY);
    })
    .await;
    let mut checks = fixture.clock.timer_checks();
    fixture.start_sweep();
    let first = fixture.next_pass().await;
    let due = first.next_pass_at.unwrap_or_else(|| panic!("next pass"));
    let started = first.ran_at.unwrap_or_else(|| panic!("ran at"));
    let until_due = due.duration_since(started).unwrap_or_default();
    assert!(until_due > DAY && until_due <= 2 * DAY + Duration::from_secs(1));

    // The one timer the sweep arms is for the due moment, not a fixed tick.
    let armed = next_check(&mut checks).await;
    assert_eq!(
        armed,
        TimerCheck {
            deadline: due,
            seen_at: started,
            fired: false
        }
    );
    // Just before the due moment the timer looks again and keeps waiting.
    fixture.clock.advance(until_due - Duration::from_secs(60));
    let early = next_check(&mut checks).await;
    assert_eq!((early.deadline, early.fired), (due, false));
    // At the due moment it fires, and exactly one pass runs then.
    fixture.clock.advance(Duration::from_secs(60));
    let at_due = next_check(&mut checks).await;
    assert_eq!(
        (at_due.deadline, at_due.seen_at, at_due.fired),
        (due, due, true)
    );
    let second = fixture.next_pass().await;

    assert_eq!(
        second.ran_at,
        Some(due),
        "the timer fires only at the due moment"
    );
    assert_eq!(second.removed, vec!["due-in-two-days".to_owned()]);
    fixture.stop().await;
}

#[tokio::test]
async fn use_recorded_inside_the_window_survives_a_host_restart() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("root: {error}"));
    private(root.path());
    let folder = root.path().join("scratch/session-a");
    // Before the restart: a session used the folder 10 days ago.
    codex_native_integration::prepare_scratch_scope(&folder)
        .unwrap_or_else(|error| panic!("prepare: {error}"));
    let now = SystemTime::now();
    set_changed(
        &folder.join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME),
        now - 10 * DAY,
    );
    set_changed(&folder, now - 40 * DAY);
    let first = start_runtime(root.path()).await;
    first
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));

    // After the restart the register is empty; the folder's own record keeps it.
    let mut runtime = start_runtime(root.path()).await;
    let clock = ManualClock::starting_now();
    let (sender, mut passes) = mpsc::unbounded_channel();
    runtime
        .start_scratch_sweep(
            Arc::clone(&clock) as Arc<dyn ScratchSweepClock>,
            Arc::new(ObservedEvents {
                passes: sender,
                hold_after_recheck: Mutex::new(None),
            }),
        )
        .unwrap_or_else(|error| panic!("sweep: {error}"));
    let pass = tokio::time::timeout(Duration::from_secs(10), passes.recv())
        .await
        .unwrap_or_else(|_| panic!("no pass"))
        .unwrap_or_else(|| panic!("sweep stopped"));

    assert!(folder.is_dir());
    assert_eq!(pass.kept_recent, vec!["session-a".to_owned()]);
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
}

#[tokio::test]
async fn a_creator_arriving_between_recheck_and_rename_keeps_a_usable_folder() {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "contested", now, 31 * DAY);
    })
    .await;
    let (reached_sender, reached) = std_mpsc::channel();
    let (release, release_receiver) = std_mpsc::channel();
    *fixture
        .events
        .hold_after_recheck
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(("contested".to_owned(), reached_sender, release_receiver));
    let folder = fixture.folder("contested");
    fixture.start_sweep();

    // The sweep holds the scope exclusively and has rechecked it.
    tokio::task::spawn_blocking(move || reached.recv())
        .await
        .unwrap_or_else(|error| panic!("join: {error}"))
        .unwrap_or_else(|error| panic!("recheck not reached: {error}"));
    let held =
        codex_native_integration::try_lock_scratch_scope_exclusively(&fixture.scratch, "contested")
            .unwrap_or_else(|error| panic!("probe: {error}"));
    assert!(
        held.is_none(),
        "the sweep holds the scope while it rechecks and renames"
    );
    // A real creator arrives now: it waits for the sweep, then gets a fresh folder.
    let creator_folder = folder.clone();
    let creator = tokio::task::spawn_blocking(move || {
        codex_native_integration::prepare_scratch_scope(&creator_folder)
    });
    let _ = release.send(());
    creator
        .await
        .unwrap_or_else(|error| panic!("join: {error}"))
        .unwrap_or_else(|error| panic!("creator: {error}"));
    let pass = fixture.next_pass().await;

    assert_eq!(pass.removed, vec!["contested".to_owned()]);
    assert!(
        folder.is_dir(),
        "the creator's folder exists after the sweep"
    );
    assert!(
        folder
            .join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME)
            .is_file()
    );
    assert!(
        !folder.join("notes.md").exists(),
        "the old contents went with the swept folder"
    );
    std::fs::write(folder.join("new.txt"), b"usable")
        .unwrap_or_else(|error| panic!("the creator's folder is writable: {error}"));
    fixture.stop().await;
}

#[tokio::test]
async fn a_failed_removal_is_reported_and_retried_the_next_day() {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "stubborn", now, 31 * DAY);
        let locked = scratch.join("stubborn/locked");
        private(&locked);
        std::fs::write(locked.join("inside.txt"), b"x").unwrap_or_else(|error| panic!("{error}"));
        set_changed(&locked.join("inside.txt"), now - 31 * DAY);
        set_changed(&locked, now - 31 * DAY);
        set_changed(&scratch.join("stubborn"), now - 31 * DAY);
    })
    .await;
    // The folder can be renamed but its contents can't be deleted.
    let locked = fixture.folder("stubborn/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500))
        .unwrap_or_else(|error| panic!("lock contents: {error}"));
    fixture.start_sweep();

    let first = fixture.next_pass().await;
    assert_eq!(first.failures.len(), 1, "{:?}", first.failures);
    assert_eq!(first.failures[0].scope, "stubborn");
    let ran = first.ran_at.unwrap_or_else(|| panic!("ran at"));
    assert_eq!(first.next_pass_at, Some(ran + DAY), "retried the next day");
    let hidden = std::fs::read_dir(&fixture.scratch)
        .unwrap_or_else(|error| panic!("{error}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".removing-stubborn-"))
        })
        .unwrap_or_else(|| panic!("the renamed folder waits for the retry"));
    std::fs::set_permissions(
        hidden.join("locked"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap_or_else(|error| panic!("unlock: {error}"));

    fixture.clock.advance(DAY);
    let second = fixture.next_pass().await;

    let hidden_name = hidden
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("name"))
        .to_owned();
    assert_eq!(second.removed, vec![hidden_name]);
    assert!(second.failures.is_empty(), "{:?}", second.failures);
    assert!(!hidden.exists());
    fixture.stop().await;
}

#[tokio::test]
async fn a_folder_last_written_31_days_ago_whose_quiet_session_closed_today_stays() {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "quiet", now, 31 * DAY);
    })
    .await;
    let folder = fixture.folder("quiet");
    let open = Arc::clone(&fixture.runtime.open_scratch_scopes);
    open.opened(&folder);
    open.closed(&folder);
    fixture.start_sweep();

    let pass = fixture.next_pass().await;

    assert!(folder.is_dir());
    assert_eq!(pass.kept_recent, vec!["quiet".to_owned()]);
    fixture.stop().await;
}

#[tokio::test]
async fn a_folder_whose_use_cannot_be_read_is_kept_reported_and_retried_the_next_day() {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "unreadable", now, 40 * DAY);
        let hidden = scratch.join("unreadable/private");
        private(&hidden);
        std::fs::write(hidden.join("inside.txt"), b"x").unwrap_or_else(|error| panic!("{error}"));
        set_changed(&hidden.join("inside.txt"), now - 40 * DAY);
        set_changed(&hidden, now - 40 * DAY);
        set_changed(&scratch.join("unreadable"), now - 40 * DAY);
    })
    .await;
    let hidden = fixture.folder("unreadable/private");
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|error| panic!("hide contents: {error}"));
    fixture.start_sweep();

    let pass = fixture.next_pass().await;

    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("restore for cleanup: {error}"));
    assert!(
        fixture.folder("unreadable").is_dir(),
        "unknown use keeps the folder"
    );
    assert!(hidden.join("inside.txt").is_file());
    assert!(pass.removed.is_empty(), "{:?}", pass.removed);
    assert_eq!(pass.failures.len(), 1, "{:?}", pass.failures);
    assert_eq!(pass.failures[0].scope, "unreadable");
    let ran = pass.ran_at.unwrap_or_else(|| panic!("ran at"));
    assert_eq!(pass.next_pass_at, Some(ran + DAY), "retried the next day");
    fixture.stop().await;
}
