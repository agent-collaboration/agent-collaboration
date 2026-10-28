//! The record choice survives reload, contention and corrupt native inputs.
use super::*;
use std::{
    collections::BTreeSet,
    sync::{Arc, Barrier},
};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn install_attempt_reloads_the_same_choice_and_uses_a_fresh_choice_for_another_epoch() -> TestResult
{
    let root = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(2)?;
    let original = load_or_record(root.path(), &project, epoch)?;
    assert_eq!(load_or_record(root.path(), &project, epoch)?, original);
    assert_ne!(
        load_or_record(root.path(), &project, Epoch::new(3)?)?,
        original
    );
    let stored = read_record(
        &record_path(
            &root.path().join("snapshot-install-attempts"),
            &project,
            epoch,
        ),
        &project,
        epoch,
    )?;
    assert_eq!(stored.attempt, original);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn concurrent_install_attempt_creators_all_observe_the_same_published_choice() -> TestResult {
    let root = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(2)?;
    let barrier = Arc::new(Barrier::new(6));
    let choices = std::thread::scope(|threads| -> Result<Vec<InstallAttemptId>, io::Error> {
        let mut workers = Vec::new();
        for _ in 0..6 {
            let waiting = barrier.clone();
            let project = &project;
            let root = root.path();
            workers.push(threads.spawn(move || {
                waiting.wait();
                load_or_record(root, project, epoch)
            }));
        }
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .map_err(|_| io::Error::other("attempt creator panicked"))?
            })
            .collect()
    })?;
    let choices: BTreeSet<_> = choices.into_iter().collect();
    assert_eq!(
        choices.len(),
        1,
        "competing creators replaced the recorded attempt"
    );
    assert!(choices.contains(&load_or_record(root.path(), &project, epoch)?));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn invalid_install_attempt_records_are_refused_without_replacement() -> TestResult {
    let root = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(2)?;
    let attempt = load_or_record(root.path(), &project, epoch)?;
    let path = record_path(
        &root.path().join("snapshot-install-attempts"),
        &project,
        epoch,
    );
    for corrupt in [
        serde_json::json!({"project":project,"epoch":0,"attempt":attempt}),
        serde_json::json!({"project":project,"epoch":3,"attempt":attempt}),
        serde_json::json!({"project":ProjectId::generate(),"epoch":2,"attempt":attempt}),
        serde_json::json!({"project":project,"epoch":2,"attempt":"550e8400-e29b-41d4-a716-446655440000"}),
        serde_json::json!({"project":project,"epoch":2,"attempt":attempt,"unexpected":true}),
    ] {
        let bytes = serde_json::to_vec(&corrupt)?;
        std::fs::write(&path, &bytes)?;
        assert!(load_or_record(root.path(), &project, epoch).is_err());
        assert_eq!(
            std::fs::read(&path)?,
            bytes,
            "corrupt choice was silently replaced"
        );
    }
    Ok(())
}
