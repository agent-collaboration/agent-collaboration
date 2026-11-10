//! The Host re-checks on the observation backoff only while the lifecycle journal is
//! unavailable; with a journal, completions arrive as its commits instead.
use super::{start_host, start_host_in};

#[tokio::test]
async fn a_host_with_a_journal_runs_no_observation_rechecks() {
    let fixture = start_host(false).await;

    let rechecking = fixture.runtime.journal_unavailable_rechecks.is_some();
    let journal_open = fixture.runtime.journal.is_some();
    fixture.runtime.shutdown().await.expect("Host shutdown");

    assert!(journal_open, "the fixture journal must open");
    assert!(!rechecking, "a journal that opened needs no re-check timer");
}

#[tokio::test]
async fn a_journal_that_failed_to_open_is_replaced_by_observation_rechecks() {
    let directory = tempfile::Builder::new()
        .prefix("host-journal-blocked-")
        .tempdir_in("/tmp")
        .expect("private Host root");
    // A directory where the journal file should be: SQLite cannot open it.
    std::fs::create_dir(directory.path().join("session-registry.sqlite"))
        .expect("blocked journal path");
    let fixture = start_host_in(false, directory).await;

    let rechecking = fixture.runtime.journal_unavailable_rechecks.is_some();
    let journal_missing = fixture.runtime.journal.is_none();
    fixture.runtime.shutdown().await.expect("Host shutdown");

    assert!(journal_missing, "the fixture journal must fail to open");
    assert!(
        rechecking,
        "running Runs and held Codex mail re-check on the observation backoff instead"
    );
}
