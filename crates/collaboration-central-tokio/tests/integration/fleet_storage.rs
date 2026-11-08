//! Canonical Fleet metadata and its feed commit in one owned store transaction.
#![cfg(feature = "conformance")]

mod archive_inputs;
mod archive_settlement;
mod associations;
mod claim_admission;
mod claim_settlement;
mod creation_decisions;
mod maximum_rows;
mod metadata;
mod owned_ids;
mod rollback;
use crate::native_fleet as support;
use fleet_admission::{FleetImportCounts, FleetImportResult};
use support::{Fixture, TestResult, bundle};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
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
