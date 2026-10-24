//! Canonical Fleet metadata and its feed commit in one owned store transaction.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]

#[path = "fleet_storage/archive_inputs.rs"]
mod archive_inputs;
#[path = "fleet_storage/archive_settlement.rs"]
mod archive_settlement;
#[path = "fleet_storage/associations.rs"]
mod associations;
#[path = "fleet_storage/claim_admission.rs"]
mod claim_admission;
#[path = "fleet_storage/claim_settlement.rs"]
mod claim_settlement;
#[path = "fleet_storage/creation_decisions.rs"]
mod creation_decisions;
#[path = "fleet_storage/definition_identity.rs"]
mod definition_identity;
#[path = "fleet_storage/definition_validation.rs"]
mod definition_validation;
#[path = "fleet_storage/lease_version_publication.rs"]
mod lease_version_publication;
#[path = "fleet_storage/maximum_rows.rs"]
mod maximum_rows;
#[path = "fleet_storage/metadata.rs"]
mod metadata;
#[path = "fleet_storage/owned_ids.rs"]
mod owned_ids;
#[path = "fleet_storage/rollback.rs"]
mod rollback;
#[path = "fleet_storage/support.rs"]
mod support;
#[path = "fleet_storage/upgrade.rs"]
mod upgrade;
use fleet_admission::{FleetImportCounts, FleetImportResult};
use support::{Fixture, TestResult, bundle};

#[tokio::test]
async fn imported_fleet_records_keep_provenance_unknown_time_digest_and_replay_after_reopen()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let offered = bundle()?;
    let proof = async {
        assert_eq!(
            fixture.import(&offered).await?,
            FleetImportResult::Imported {
                counts: FleetImportCounts {
                    workspaces: 1,
                    boards: 1,
                    directory_entries: 1
                },
            }
        );
        assert_eq!(fixture.counts().await?, vec![1, 1, 1, 1, 3]);
        fixture.assert_import_headers().await?;
        assert_eq!(
            fixture.import(&offered).await?,
            FleetImportResult::AlreadyImported
        );
        assert_eq!(fixture.counts().await?, vec![1, 1, 1, 1, 3]);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fixture.objects.close().await;
    closed?;
    proof?;
    drop(fixture);
    let reopened = Fixture::open(root.path()).await?;
    let proof = async {
        assert_eq!(
            reopened.import(&offered).await?,
            FleetImportResult::AlreadyImported
        );
        assert_eq!(reopened.counts().await?, vec![1, 1, 1, 1, 3]);
        reopened.assert_import_headers().await
    }
    .await;
    let closed = reopened.objects.close().await;
    closed?;
    proof
}
