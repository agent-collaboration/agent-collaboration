use super::{CopyFleetReferencePort, CopyFleetReferenceRefusal, InMemoryCopyFleetReferences};
use fleet_identity_protocol::OrganizationId;
use fleet_sync_protocol::DatabaseId;

fn project(value: &str) -> DatabaseId {
    value.parse().unwrap()
}
fn organization(value: &str) -> OrganizationId {
    value.to_owned().try_into().unwrap()
}
const FIRST: &str = "019f0000-0000-7000-8000-00000000ad01";
const SECOND: &str = "019f0000-0000-7000-8000-00000000ad02";
const PROJECT: &str = "019f0000-0000-7000-8000-00000000ad03";
const OTHER_PROJECT: &str = "019f0000-0000-7000-8000-00000000ad04";

#[tokio::test(start_paused = true)]
async fn copy_fleet_reference_is_missing_until_the_admitting_fleet_binds_it() {
    let references = InMemoryCopyFleetReferences::default();
    assert_eq!(
        references.resolve(project(PROJECT)).await,
        Err(CopyFleetReferenceRefusal::Missing)
    );
    references
        .bind(project(PROJECT), organization(FIRST))
        .await
        .unwrap();
    assert_eq!(
        references.resolve(project(PROJECT)).await.unwrap(),
        organization(FIRST)
    );
}

#[tokio::test(start_paused = true)]
async fn copy_fleet_reference_repeats_are_safe_but_another_fleet_cannot_take_it() {
    let references = InMemoryCopyFleetReferences::default();
    references
        .bind(project(PROJECT), organization(FIRST))
        .await
        .unwrap();
    references
        .bind(project(PROJECT), organization(FIRST))
        .await
        .unwrap();

    let result = references
        .bind(project(PROJECT), organization(SECOND))
        .await;

    assert_eq!(result, Err(CopyFleetReferenceRefusal::OrganizationMismatch));
    assert_eq!(
        references.resolve(project(PROJECT)).await.unwrap(),
        organization(FIRST)
    );
}

#[tokio::test(start_paused = true)]
async fn copy_fleet_reference_keeps_distinct_projects_and_fleets_separate() {
    let references = InMemoryCopyFleetReferences::default();
    references
        .bind(project(PROJECT), organization(FIRST))
        .await
        .unwrap();
    references
        .bind(project(OTHER_PROJECT), organization(SECOND))
        .await
        .unwrap();
    assert_eq!(
        references.resolve(project(PROJECT)).await.unwrap(),
        organization(FIRST)
    );
    assert_eq!(
        references.resolve(project(OTHER_PROJECT)).await.unwrap(),
        organization(SECOND)
    );
}

#[tokio::test(start_paused = true)]
async fn copy_fleet_reference_stand_in_does_not_claim_process_restart_durability() {
    // Catch a global/static map leaking a previous host's bindings into a new host.
    let before = InMemoryCopyFleetReferences::default();
    before
        .bind(project(PROJECT), organization(FIRST))
        .await
        .unwrap();
    let restarted = InMemoryCopyFleetReferences::default();
    assert_eq!(
        restarted.resolve(project(PROJECT)).await,
        Err(CopyFleetReferenceRefusal::Missing)
    );
}
