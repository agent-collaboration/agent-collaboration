//! Equal versions identify committed lease facts, independently of display time.
#![allow(clippy::panic_in_result_fn)]
use super::*;
use fleet_admission::{DirectoryLease, DirectoryLeaseState};

type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

fn held_metadata() -> TestResult<LeaseMetadata> {
    Ok(LeaseMetadata {
        lease: DirectoryLease::Held {
            epoch: 3.try_into()?,
            holder: "550e8400-e29b-41d4-a716-446655440001"
                .to_owned()
                .try_into()?,
            state: DirectoryLeaseState::Held,
            expires_at: "2026-10-22T10:00:30Z".parse()?,
        },
        lease_version: message_board::LeaseVersion::new(
            3.try_into()?,
            message_board::RegistryIncarnation::new(1)?,
            2,
        )?,
        as_of: "2026-10-22T10:00:00Z".parse()?,
    })
}

struct CountedUnavailableSource(std::sync::atomic::AtomicUsize);

impl ProjectReadCopySource for CountedUnavailableSource {
    fn pull<'a>(&'a self, _: &'a ProjectId) -> ProjectReadCopyFuture<'a> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Err(CentralReadError::Unavailable) })
    }
}

#[tokio::test]
async fn completed_authority_never_pulls_or_initializes_a_read_slot() -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let source = Arc::new(CountedUnavailableSource(AtomicUsize::new(0)));
    let reads = CentralProjectReads::new(source.clone());
    let project = ProjectId::generate();
    assert_eq!(reads.completed_authority(&project).await?, None);
    assert!(reads.projects.lock().await.is_empty());
    let slot = Arc::new(ProjectReadSlot::default());
    reads
        .projects
        .lock()
        .await
        .insert(project.clone(), slot.clone());
    assert_eq!(reads.completed_authority(&project).await?, None);
    assert_eq!(source.0.load(Ordering::SeqCst), 0);
    let state = slot.state.read().await;
    assert_eq!(state.attempt, 0);
    assert!(state.completed.is_none() && state.failure.is_none());
    Ok(())
}

#[test]
fn equal_version_refresh_publishes_the_incoming_expiry_display() -> TestResult {
    let held = held_metadata()?;
    let mut expired = held.clone();
    let DirectoryLease::Held { state, .. } = &mut expired.lease else {
        return Err("fixture must start with a Held lease".into());
    };
    *state = DirectoryLeaseState::Expired;
    assert_eq!(
        select_lease(
            Some(&DirectorySide::Known(held)),
            DirectorySide::Known(expired.clone())
        )?,
        DirectorySide::Known(expired),
        "display refresh must retain the incoming value, not the old Held label"
    );
    Ok(())
}

#[test]
fn equal_version_conflicting_committed_lease_facts_are_corrupt() -> TestResult {
    let stored = held_metadata()?;
    let mut holder = stored.clone();
    let DirectoryLease::Held { holder: value, .. } = &mut holder.lease else {
        return Err("fixture must supply a holder".into());
    };
    *value = "550e8400-e29b-41d4-a716-446655440002"
        .to_owned()
        .try_into()?;
    let mut epoch = stored.clone();
    let DirectoryLease::Held { epoch: value, .. } = &mut epoch.lease else {
        return Err("fixture must supply an epoch".into());
    };
    *value = 4.try_into()?;
    let mut expiry = stored.clone();
    let DirectoryLease::Held { expires_at, .. } = &mut expiry.lease else {
        return Err("fixture must supply an expiry".into());
    };
    *expires_at += chrono::Duration::milliseconds(1);
    let mut observation = stored.clone();
    observation.as_of += chrono::Duration::milliseconds(1);
    let mut unheld = stored.clone();
    unheld.lease = DirectoryLease::Unheld;
    let mut retired = stored.clone();
    retired.lease = DirectoryLease::Retired {
        retired_at: stored.as_of,
    };
    for (field, conflicting) in [
        ("holder", holder),
        ("epoch", epoch),
        ("expires_at", expiry),
        ("as_of", observation),
        ("Held/Unheld", unheld),
        ("Held/Retired", retired),
    ] {
        assert!(
            matches!(
                select_lease(
                    Some(&DirectorySide::Known(stored.clone())),
                    DirectorySide::Known(conflicting)
                ),
                Err(CentralReadError::Corrupt)
            ),
            "equal version accepted conflicting {field}"
        );
    }
    let mut unheld = stored.clone();
    unheld.lease = DirectoryLease::Unheld;
    assert_eq!(
        select_lease(
            Some(&DirectorySide::Known(unheld.clone())),
            DirectorySide::Known(unheld.clone())
        )?,
        DirectorySide::Known(unheld.clone())
    );
    assert!(matches!(
        select_lease(
            Some(&DirectorySide::Known(unheld)),
            DirectorySide::Known(stored)
        ),
        Err(CentralReadError::Corrupt)
    ));
    Ok(())
}
