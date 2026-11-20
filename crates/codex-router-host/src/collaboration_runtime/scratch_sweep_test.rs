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
async fn the_timer_fires_at_the_earliest_due_moment_and_never_later_than_29_days_while_a_scope_is_open()
 {
    // Arrange: one folder due in two days, and one a session holds open.
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "due-in-two-days", now, 28 * DAY);
        seed_scope(scratch, "held-open", now, 40 * DAY);
    })
    .await;
    let held = fixture.folder("held-open");
    fixture.runtime.open_scratch_scopes.opened(&held);
    let mut checks = fixture.clock.timer_checks();
    fixture.start_sweep();
    let first = fixture.next_pass().await;
    let due = first.next_pass_at.unwrap_or_else(|| panic!("next pass"));
    let started = first.ran_at.unwrap_or_else(|| panic!("ran at"));
    let until_due = due.duration_since(started).unwrap_or_default();
    assert!(until_due > DAY && until_due <= 2 * DAY + Duration::from_secs(1));
    assert_eq!(
        next_check(&mut checks).await,
        TimerCheck {
            deadline: due,
            seen_at: started,
            fired: false
        },
        "the one timer is armed for the earliest due moment"
    );

    // Act 1: the due moment comes; the due folder goes and only the open scope is left.
    fixture.clock.advance(until_due);
    assert!(next_check(&mut checks).await.fired);
    let second = fixture.next_pass().await;
    let second_ran = second.ran_at.unwrap_or_else(|| panic!("ran at"));
    let restamp_due = second_ran + 29 * DAY;

    // Assert 1: the next pass is 29 days away, a day before the open scope's
    // freshly recorded use could make it eligible.
    assert_eq!(second.removed, vec!["due-in-two-days".to_owned()]);
    assert_eq!(second.kept_open, vec!["held-open".to_owned()]);
    assert_eq!(
        last_used(&held),
        second_ran,
        "use recorded at the sweep's time"
    );
    assert_eq!(second.next_pass_at, Some(restamp_due));
    assert_eq!(
        next_check(&mut checks).await,
        TimerCheck {
            deadline: restamp_due,
            seen_at: second_ran,
            fired: false
        }
    );

    // Act 2: a minute short of 29 days the timer keeps waiting; at 29 days it fires.
    fixture.clock.advance(29 * DAY - Duration::from_secs(60));
    let early = next_check(&mut checks).await;
    fixture.clock.advance(Duration::from_secs(60));
    let at_due = next_check(&mut checks).await;
    let third = fixture.next_pass().await;

    // Assert 2: the pass re-records the open scope's use then.
    assert_eq!((early.deadline, early.fired), (restamp_due, false));
    assert_eq!((at_due.deadline, at_due.fired), (restamp_due, true));
    assert_eq!(third.ran_at, Some(restamp_due));
    assert_eq!(third.kept_open, vec!["held-open".to_owned()]);
    assert_eq!(last_used(&held), restamp_due);
    assert_eq!(third.next_pass_at, Some(restamp_due + 29 * DAY));
    fixture.stop().await;
}

#[tokio::test]
async fn after_a_pass_with_nothing_open_the_timer_waits_at_most_29_days_and_records_a_scope_opened_since()
 {
    // Arrange: the first pass finds no folders and nothing open.
    let mut fixture = Fixture::start(|_, _| {}).await;
    let mut checks = fixture.clock.timer_checks();
    fixture.start_sweep();
    let empty = fixture.next_pass().await;
    let empty_ran = empty.ran_at.unwrap_or_else(|| panic!("ran at"));
    let armed = next_check(&mut checks).await;
    // A session opens a scope after that pass and leaves it quiet.
    let folder = fixture.folder("opened-later");
    fixture.runtime.open_scratch_scopes.opened(&folder);
    std::fs::write(folder.join("notes.md"), b"scratch")
        .unwrap_or_else(|error| panic!("note: {error}"));

    // Act: the clock reaches the armed deadline.
    fixture
        .clock
        .advance(armed.deadline.duration_since(empty_ran).unwrap_or_default());
    let fired = next_check(&mut checks).await;
    let next = fixture.next_pass().await;

    // Assert: the timer waited no longer than 29 days, and that pass recorded the
    // later scope's use at its own time.
    assert!(
        empty.removed.is_empty() && empty.kept_open.is_empty(),
        "{empty:?}"
    );
    assert!(!armed.fired, "{armed:?}");
    assert!(
        armed.deadline <= empty_ran + 29 * DAY,
        "an empty pass must not wait longer than 29 days: armed {:?} after {:?}",
        armed.deadline.duration_since(empty_ran),
        empty_ran
    );
    assert!(fired.fired, "{fired:?}");
    assert_eq!(next.ran_at, Some(armed.deadline));
    assert_eq!(next.kept_open, vec!["opened-later".to_owned()]);
    assert_eq!(last_used(&folder), armed.deadline);
    assert!(folder.join("notes.md").is_file());
    fixture.stop().await;
}

/// Starts `runtime`'s sweep on `clock`; each finished pass arrives on the receiver.
fn start_observed_sweep(
    runtime: &mut CollaborationRuntime,
    clock: &Arc<ManualClock>,
) -> mpsc::UnboundedReceiver<ScratchSweepReport> {
    let (sender, passes) = mpsc::unbounded_channel();
    runtime
        .start_scratch_sweep(
            Arc::clone(clock) as Arc<dyn ScratchSweepClock>,
            Arc::new(ObservedEvents {
                passes: sender,
                hold_after_recheck: Mutex::new(None),
            }),
        )
        .unwrap_or_else(|error| panic!("sweep: {error}"));
    passes
}

async fn next_report(
    passes: &mut mpsc::UnboundedReceiver<ScratchSweepReport>,
) -> ScratchSweepReport {
    tokio::time::timeout(Duration::from_secs(10), passes.recv())
        .await
        .unwrap_or_else(|_| panic!("no sweep pass reported"))
        .unwrap_or_else(|| panic!("sweep stopped"))
}

#[tokio::test]
async fn an_open_scope_whose_use_was_recorded_before_a_crash_stays_after_a_restart_past_30_days() {
    // Arrange: a quiet session holds a folder open from day 0; the pass on day 29
    // records its use.
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("root: {error}"));
    private(root.path());
    let scratch = root.path().join("scratch");
    private(&scratch);
    let clock = ManualClock::starting_now();
    let day_0 = clock.now();
    seed_scope(&scratch, "quiet-open", day_0, 40 * DAY);
    let folder = scratch.join("quiet-open");
    let mut before_crash = start_runtime(root.path()).await;
    before_crash.open_scratch_scopes.opened(&folder);
    let mut passes = start_observed_sweep(&mut before_crash, &clock);
    let opened_pass = next_report(&mut passes).await;
    clock.advance(29 * DAY);
    let restamp_pass = next_report(&mut passes).await;
    let day_29 = day_0 + 29 * DAY;
    let recorded_before_crash = last_used(&folder);
    // The Host stops before the session closes: its register of open scopes is gone.
    before_crash
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));

    // Act: the Host restarts on day 31, before the session is opened again.
    let mut after_restart = start_runtime(root.path()).await;
    let restart_clock = ManualClock::starting_now();
    restart_clock.advance(31 * DAY);
    let mut restart_passes = start_observed_sweep(&mut after_restart, &restart_clock);
    let after = next_report(&mut restart_passes).await;

    // Assert
    assert_eq!(opened_pass.kept_open, vec!["quiet-open".to_owned()]);
    assert_eq!(restamp_pass.ran_at, Some(day_29), "a pass ran on day 29");
    assert_eq!(restamp_pass.kept_open, vec!["quiet-open".to_owned()]);
    assert_eq!(
        recorded_before_crash, day_29,
        "its use was recorded on day 29"
    );
    assert!(after.removed.is_empty(), "{:?}", after.removed);
    assert_eq!(after.kept_recent, vec!["quiet-open".to_owned()]);
    assert!(folder.join("notes.md").is_file(), "its contents are kept");
    after_restart
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
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

/// Reads a scope's recorded last use.
fn last_used(folder: &Path) -> SystemTime {
    std::fs::metadata(folder.join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME))
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|error| panic!("use stamp {}: {error}", folder.display()))
}

#[tokio::test]
async fn a_folder_with_a_session_held_open_for_31_quiet_days_stays_and_its_use_is_recorded() {
    let mut fixture = Fixture::start(|scratch, now| {
        seed_scope(scratch, "held-open", now, 40 * DAY);
    })
    .await;
    let folder = fixture.folder("held-open");
    let open = Arc::clone(&fixture.runtime.open_scratch_scopes);
    open.opened(&folder);
    fixture.start_sweep();
    let first = fixture.next_pass().await;
    // The use stamp is now 31 days old by the sweep's clock.
    let stamped_before = last_used(&folder);
    set_changed(
        &folder.join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME),
        stamped_before - DAY,
    );

    fixture.clock.advance(31 * DAY);
    let second = fixture.next_pass().await;

    assert!(
        folder.join("notes.md").is_file(),
        "an open session keeps its folder"
    );
    assert_eq!(second.kept_open, vec!["held-open".to_owned()]);
    assert!(second.removed.is_empty(), "{:?}", second.removed);
    assert!(
        last_used(&folder) > stamped_before - DAY,
        "the pass records the open session's use"
    );
    assert_eq!(first.kept_open, vec!["held-open".to_owned()]);
    fixture.stop().await;
}

#[tokio::test]
async fn a_thread_folder_stays_for_31_quiet_days_while_one_of_two_sessions_remains_open() {
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

    fixture.clock.advance(31 * DAY);
    let second = fixture.next_pass().await;

    assert!(
        folder.join("notes.md").is_file(),
        "the remaining session keeps the folder"
    );
    assert_eq!(second.kept_open, vec!["thread-root".to_owned()]);
    assert!(second.removed.is_empty(), "{:?}", second.removed);
    assert_eq!(first.kept_open, vec!["thread-root".to_owned()]);
    fixture.stop().await;
}

/// One logged event: its level and its fields as text.
type CapturedEvent = (tracing::Level, std::collections::BTreeMap<String, String>);

/// Every event logged while the capture is the subscriber.
#[derive(Clone, Default)]
struct CapturedEvents(Arc<Mutex<Vec<CapturedEvent>>>);

struct FieldText<'a>(&'a mut std::collections::BTreeMap<String, String>);

impl tracing::field::Visit for FieldText<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CapturedEvents {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = std::collections::BTreeMap::new();
        event.record(&mut FieldText(&mut fields));
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((*event.metadata().level(), fields));
    }
}

#[test]
fn a_failed_removal_is_logged_as_a_warning_naming_the_scope_and_error() {
    use tracing_subscriber::prelude::*;
    let captured = CapturedEvents::default();
    let report = ScratchSweepReport {
        ran_at: Some(SystemTime::UNIX_EPOCH),
        failures: vec![ScratchSweepFailure {
            scope: "stubborn".to_owned(),
            error: io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied"),
        }],
        next_pass_at: Some(SystemTime::UNIX_EPOCH + DAY),
        ..ScratchSweepReport::default()
    };

    tracing::subscriber::with_default(
        tracing_subscriber::registry().with(captured.clone()),
        || {
            LoggedScratchSweepEvents.pass_finished(&report);
        },
    );

    let events = captured
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let warning = events
        .iter()
        .find(|(level, _)| *level == tracing::Level::WARN)
        .map(|(_, fields)| fields.clone())
        .unwrap_or_else(|| panic!("a failed removal must be logged as a warning: {events:?}"));
    assert_eq!(warning.get("scope").map(String::as_str), Some("stubborn"));
    assert_eq!(
        warning.get("error").map(String::as_str),
        Some("Permission denied")
    );
    assert_eq!(
        warning.get("message").map(String::as_str),
        Some("scratch folder kept; retried the next day")
    );
}
