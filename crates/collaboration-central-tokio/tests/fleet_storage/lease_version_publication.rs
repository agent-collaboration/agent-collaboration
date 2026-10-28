//! Real Fleet publication keeps Registry versions independent of Copy positions.
use super::{
    claim_settlement::{acquire_incarnation, encoded_revision, registry_call},
    support::{Fixture, TestResult, bundle},
};
use board_sync_protocol::BoardReadPage;
use fleet_admission::{DirectoryEntry, DirectoryLease, DirectorySide};
use message_board::{LeaseVersion, RegistryIncarnation};
use serde_json::{Value, json};

const PROJECT: &str = "019f0000-0000-7000-8000-000000000a83";
const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400a9";
const PEER: &str = "019f0000-0000-7000-8000-000000000b03";

async fn directory(fixture: &Fixture) -> TestResult<DirectoryEntry> {
    let answer = fixture.runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"connection","value":SERVICE},"sourceFace":"session","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"directory_version"},
        "kind":"central.fleet-read","target":null,"request":{"kind":"directory","value":{"query":{}}}
    }))?).await.map_err(|error|format!("real Fleet directory: {error:?}"))?;
    let BoardReadPage::Directory { entries, .. } = serde_json::from_slice(&answer)? else {
        return Err("directory answered another page family".into());
    };
    let [entry] = entries.as_slice() else {
        return Err("expected one imported project".into());
    };
    Ok(entry.clone().decode()?)
}

async fn publish(fixture: &Fixture, projection: Value) -> TestResult<Value> {
    let answer = fixture
        .runtime
        .on_call(registry_call(
            "central.lease-projection",
            projection,
            durable_steps::Uuid::now_v7(),
        )?)
        .await
        .map_err(|error| format!("real lease publication: {error:?}"))?;
    Ok(serde_json::from_slice(&answer)?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn directory_keeps_epoch_first_registry_version_across_late_repeated_and_reopened_reads()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        fixture.import(&bundle()?).await?;
        assert_eq!(directory(&fixture).await?.lease_side,DirectorySide::NotYetReported);
        let incarnation=acquire_incarnation(&fixture).await?;
        assert_eq!(incarnation.value(),1);
        // Supply the Copy's separate committed summary through its real object entry.
        // Copy epoch9 deliberately disagrees with both the lease epochs2/3 and fence1.
        fixture.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"copy","value":PROJECT},"sourceFace":"copy","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"copy_directory_version"},
            "kind":"central.directory-summary","target":null,"request":{
                "database":PROJECT,"position":{"epoch":9,"sequence":50},"ackedAt":1_792_411_199_000_i64,
                "summary":{"kind":"projectSummary","value":{"latestRecordAt":{"kind":"legacyUnknown"}}}
            }
        }))?).await.map_err(|error|format!("Copy summary: {error:?}"))?;
        let old=encoded_revision(collaboration_central_core::RegistryIncarnation::new(99)?,2,99,SERVICE,PEER,1_792_411_200_000)?;
        assert_eq!(publish(&fixture,old.clone()).await?,json!({"kind":"applied"}));
        let latest=encoded_revision(incarnation,3,1,SERVICE,PEER,1_792_411_201_000)?;
        assert_eq!(publish(&fixture,latest.clone()).await?,json!({"kind":"applied"}));
        let current=directory(&fixture).await?;
        let DirectorySide::Known(metadata)=&current.lease_side else {return Err("lease publication absent".into());};
        assert_eq!(metadata.lease_version,LeaseVersion::new(3.try_into()?,RegistryIncarnation::new(1)?,1)?);
        assert!(matches!(&metadata.lease,DirectoryLease::Held {epoch,holder,..} if epoch.value()==3 && holder.as_str()==SERVICE));
        assert_eq!(metadata.as_of.timestamp_millis(),1_792_411_201_000);
        let DirectorySide::Known(copy)=&current.copy_side else {return Err("separate Copy summary absent".into());};
        assert_eq!(copy.as_of.position.epoch.value(),9);
        assert_eq!(publish(&fixture,old).await?,json!({"kind":"older","value":{"stored":{"epoch":3,"incarnation":1,"change":1}}}));
        assert_eq!(directory(&fixture).await?,current,"late lower epoch must not move the view backwards");
        assert_eq!(publish(&fixture,latest).await?,json!({"kind":"duplicate"}));
        assert_eq!(directory(&fixture).await?,current,"repeated observation must preserve version and source clock");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(current)
    }.await;
    fixture.objects.close().await?;
    drop(fixture);
    let expected = proof?;
    let reopened = Fixture::open(root.path()).await?;
    let observed = directory(&reopened).await;
    reopened.objects.close().await?;
    assert_eq!(observed?, expected);
    Ok(())
}
