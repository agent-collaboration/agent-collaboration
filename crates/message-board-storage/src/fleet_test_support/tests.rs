use super::login_repository_contract::{candidate_fixture, expected_login};
use super::*;
use std::{future::poll_fn, task::Poll};

// Wrong behaviors: early/defaulted environment, mixed replacement fields,
// cross-organization replacement or an unfenced stale delete. Oracle: PD
// bb4c7145 and the repository ruling. Real store must run this same suite later.
#[tokio::test]
async fn fleet_login_repository_memory_honors_the_shared_contract() {
    let repository = MemoryFleetLoginRepository::new();
    verify_login_repository_contract(&repository).await.unwrap();
}

struct SplitReadWriteRepository(MemoryFleetLoginRepository);

impl FleetLoginRepository for SplitReadWriteRepository {
    fn load_fleet_login(&self) -> FleetLoginStorageRequest<'_, Option<SavedFleetLogin>> {
        self.0.load_fleet_login()
    }
    fn delete_fleet_login<'a>(
        &'a self,
        expected: &'a EnrollmentId,
    ) -> FleetLoginStorageRequest<'a, bool> {
        self.0.delete_fleet_login(expected)
    }
    fn replace_fleet_login<'a>(
        &'a self,
        candidate: &'a FleetLoginCandidate,
    ) -> FleetLoginStorageRequest<'a, SavedFleetLogin> {
        Box::pin(async move {
            let previous = self.0.load_fleet_login().await?;
            if let Some(previous) = &previous
                && previous.organization_id != candidate.organization_id
            {
                return Err(FleetLoginStorageFailure::OrganizationMismatch {
                    organization_id: previous.organization_id.clone(),
                });
            }
            // Deliberately wrong: release the read lease and force both racing
            // calls to snapshot the old row before either replacement writes.
            let mut yielded = false;
            poll_fn(move |context| {
                if yielded {
                    Poll::Ready(())
                } else {
                    yielded = true;
                    context.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            let environment = candidate
                .requested_machine_environment
                .clone()
                .or_else(|| previous.map(|saved| saved.machine_environment))
                .unwrap_or(MachineEnvironment::Physical);
            let saved = expected_login(candidate, environment);
            *self
                .0
                .saved
                .lock()
                .map_err(|_| FleetLoginStorageFailure::Unavailable)? = Some(saved.clone());
            Ok(saved)
        })
    }
}

#[tokio::test]
async fn fleet_login_repository_contract_rejects_a_split_read_replace_race() {
    let repository = SplitReadWriteRepository(MemoryFleetLoginRepository::new());
    let result = verify_login_repository_contract(&repository).await;
    assert!(matches!(
        result,
        Err(FleetLoginContractFailure::Violation(
            "racing replacements must have one whole-row serial order, including environment retention"
        ))
    ));
}

#[tokio::test]
async fn fleet_login_repository_delete_is_idempotent_and_enrollment_scoped() {
    // Arrange
    let repository = MemoryFleetLoginRepository::new();
    let candidate = candidate_fixture(0x30, None).unwrap();
    let saved = repository.replace_fleet_login(&candidate).await.unwrap();
    let other = candidate_fixture(0x31, None).unwrap();

    // Act / Assert
    assert!(
        !repository
            .delete_fleet_login(&other.enrollment_id)
            .await
            .unwrap()
    );
    assert_eq!(repository.load_fleet_login().await.unwrap(), Some(saved));
    assert!(
        repository
            .delete_fleet_login(&candidate.enrollment_id)
            .await
            .unwrap()
    );
    assert!(
        !repository
            .delete_fleet_login(&candidate.enrollment_id)
            .await
            .unwrap()
    );
}
