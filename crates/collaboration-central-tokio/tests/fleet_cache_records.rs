//! Retained cache/repository cases consume the real native Fleet owner.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]
#[path = "fleet_storage/support.rs"]
#[allow(dead_code)]
mod native_source;
#[path = "support/fleet_cache_source.rs"]
mod source;
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;
use message_board_storage::{FleetCache, RouterLocalStorageError};
use source::NativeFleetSource;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn request(
    workspace: WorkspaceId,
) -> Result<FleetRequest, Box<dyn std::error::Error + Send + Sync>> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace,
        actor: RecordedActor::declared(Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        }),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("Local".to_owned())?,
            description: Description::try_from("local stores".to_owned())?,
        },
    })
}

#[tokio::test]
async fn fleet_cache_installs_a_real_snapshot_and_rejects_a_gapped_page_atomically() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fleet = NativeFleetSource::open(&directory.path().join("central")).await?;
    let cache_path = directory.path().join("cache.sqlite");
    let cache = FleetCache::open(&cache_path).await?;
    let original = request(WorkspaceId::generate())?;
    fleet.decide(&original).await?;
    let snapshot = fleet.snapshot().await?;
    cache.replace_snapshot(&snapshot).await?;
    assert_eq!(cache.snapshot().await?, snapshot);
    let Some(mut record) = cache.record(&original.workspace.clone().into()).await? else {
        panic!("workspace missing")
    };
    let gap = FleetPosition::try_from(snapshot.through.get() + 2)?;
    let origin = record
        .envelope()
        .position
        .origin()
        .cloned()
        .ok_or("Fleet origin missing")?;
    record.envelope_mut().position = RecordPlacement::Fleet(FleetFeedEntry {
        position: gap,
        origin,
    });
    assert!(matches!(
        cache
            .apply_feed(
                snapshot.through,
                &[fleet_admission::FleetFeedEntry {
                    position: gap,
                    record
                }]
            )
            .await,
        Err(RouterLocalStorageError::PositionGap)
    ));
    assert_eq!(cache.snapshot().await?, snapshot);
    let mut rename = original.clone();
    rename.request_id = RequestId::generate();
    rename.operation = FleetOperation::WorkspaceUpdate {
        name: ResourceName::try_from("Updated through feed".to_owned())?,
        description: Description::try_from("local stores".to_owned())?,
    };
    fleet.decide(&rename).await?;
    let page = fleet.feed_fragment(snapshot.through, 1).await?;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.covered_through.get(), 3);
    assert_eq!(page.high_water.get(), 4);
    cache.apply_feed(snapshot.through, &page.entries).await?;
    assert_eq!(cache.snapshot().await?.through, page.covered_through);
    let remainder = fleet.feed_fragment(page.covered_through, 10).await?;
    cache
        .apply_feed(page.covered_through, &remainder.entries)
        .await?;
    assert_eq!(cache.snapshot().await?, fleet.snapshot().await?);
    cache.close().await;
    let reopened = FleetCache::open(&cache_path).await?;
    assert_eq!(reopened.snapshot().await?, fleet.snapshot().await?);
    reopened.close().await;
    fleet.close().await?;
    Ok(())
}

#[tokio::test]
async fn repository_attachments_travel_in_workspace_feed_cache_and_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fleet_path = directory.path().join("central");
    let cache_path = directory.path().join("cache.sqlite");
    let fleet = NativeFleetSource::open(&fleet_path).await?;
    let cache = FleetCache::open(&cache_path).await?;
    let original = request(WorkspaceId::generate())?;
    fleet.decide(&original).await?;
    cache.replace_snapshot(&fleet.snapshot().await?).await?;
    let repository: RepositoryRef = serde_json::from_value(serde_json::json!({
        "kind":"origin", "normalizedOrigin":"github.com/example/repository"
    }))?;
    for attached in [true, false] {
        let mut mutation = original.clone();
        mutation.request_id = RequestId::generate();
        mutation.operation = if attached {
            FleetOperation::RepositoryAttach {
                repository: repository.clone(),
            }
        } else {
            FleetOperation::RepositoryDetach {
                repository: repository.clone(),
            }
        };
        let after = cache.snapshot().await?.through;
        let reply = fleet.decide(&mutation).await?;
        let expected = if attached {
            vec![repository.clone()]
        } else {
            Vec::new()
        };
        let workspace = fleet
            .record(&original.workspace.clone().into())
            .await?
            .ok_or("workspace missing")?;
        let payload = serde_json::to_value(&workspace)?;
        assert_eq!(
            payload["record"]["repositories"],
            serde_json::to_value(&expected)?
        );
        assert!(
            matches!(&reply, RequestDecision::Accepted { effects, .. } if effects.entries().contains(&(workspace.reference(), workspace.envelope().position.clone())))
        );
        let page = fleet.feed_fragment(after, 100).await?;
        assert!(page.entries.iter().any(|entry| entry.record == workspace));
        cache.apply_feed(after, &page.entries).await?;
        assert_eq!(
            cache.record(&original.workspace.clone().into()).await?,
            Some(workspace)
        );
        let before_replay = fleet.snapshot().await?;
        assert_eq!(fleet.decide(&mutation).await?, reply);
        assert_eq!(fleet.snapshot().await?, before_replay);
    }
    cache.close().await;
    fleet.close().await?;
    let reopened_fleet = NativeFleetSource::open(&fleet_path).await?;
    let reopened_cache = FleetCache::open(&cache_path).await?;
    assert_eq!(
        reopened_cache.snapshot().await?,
        reopened_fleet.snapshot().await?
    );
    reopened_cache.close().await;
    reopened_fleet.close().await?;
    Ok(())
}
