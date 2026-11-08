//! Directory lease versions stay independent of the project's reported replication position.
use super::{
    composed_fleet_fixture::*, fleet_claim_adapter_fixture::prepare_fleet,
    fleet_claim_settlement::settle,
};
use board_sync_protocol::BoardReadPage;
use fleet_admission::{DirectoryEntry, DirectoryLease, DirectorySide};
use message_board::{LeaseVersion, RegistryIncarnation};
use serde_json::{Value, json};

async fn directory(fixture: &Fixture) -> TestResult<DirectoryEntry> {
    let answer = fixture
        .call(
            "session",
            "central.fleet-read",
            Value::Null,
            json!({"kind":"directory","value":{"query":{}}}),
        )
        .await?;
    let BoardReadPage::Directory { entries, .. } = serde_json::from_value(answer)? else {
        return Err("directory returned another page family".into());
    };
    let [entry] = entries.as_slice() else {
        return Err("expected one genuine project".into());
    };
    Ok(entry.clone().decode()?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A late lower-epoch claim cannot regress the real hook view even with a larger incarnation, or borrow the Copy position as a lease version."
)]
async fn directory_keeps_epoch_first_registry_version_across_late_repeated_and_reopened_reads()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let initial = fixture.lease_and_notices().await?;
    let mut old: Value = serde_json::from_str(initial.1.as_deref().ok_or("hook view missing")?)?;
    // This is an explicit late input, not a second writer. Its valid content
    // digest excludes incarnation, so epoch must still outrank this larger value.
    old["incarnation"] = json!(99);

    // A supplied Copy summary is this Directory receiver component's input.
    // Its epoch9 deliberately disagrees with the actual project fence and both lease epochs.
    fixture.runtime.on_call(serde_json::to_vec(&json!({
        "organization":"019f0000-0000-7000-8000-00000000ac01","source":{"kind":"copy","value":PROJECT},"sourceFace":"copy","authority":{"kind":"object"},
        "destination":{"kind":"fleet","value":{"organization":"019f0000-0000-7000-8000-00000000ac01"}},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"copy_directory_version"},
        "kind":"central.directory-summary","target":null,"request":{
            "database":PROJECT,"position":{"epoch":9,"sequence":50},"ackedAt":NOW-1,
            "summary":{"kind":"projectSummary","value":{"latestRecordAt":{"kind":"legacyUnknown"}}}
        }
    }))?).await?;
    fixture.io.advance(1_000);
    let preview = fixture.owner(json!({"kind":"preview"})).await?;
    let preview = preview
        .pointer("/value/value/preview")
        .cloned()
        .ok_or("genuine takeover preview missing")?;
    let changed = fixture
        .owner(json!({"kind":"takeover","value":{
            "transferId":durable_steps::Uuid::now_v7(),"toInstallation":SERVICE_B,
            "reason":"Directory version ordering","preview":preview
        }}))
        .await?;
    assert_eq!(changed.pointer("/value/kind"), Some(&json!("granted")));
    let latest = fixture.lease_and_notices().await?;
    let current = directory(&fixture).await?;
    let DirectorySide::Known(metadata) = &current.lease_side else {
        return Err("genuine hook projection absent".into());
    };
    assert_eq!(
        metadata.lease_version,
        LeaseVersion::new(2.try_into()?, RegistryIncarnation::new(2)?, 1)?
    );
    assert!(
        matches!(&metadata.lease, DirectoryLease::Held {epoch,holder,..} if epoch.value()==2 && holder.as_str()==SERVICE_B)
    );
    assert_eq!(metadata.as_of.timestamp_millis(), NOW + 1_000);
    let DirectorySide::Known(copy) = &current.copy_side else {
        return Err("separate Copy summary absent".into());
    };
    assert_eq!(
        copy.as_of.position,
        fleet_sync_protocol::ReplicationPosition::new(
            fleet_sync_protocol::Epoch::new(9)?,
            fleet_sync_protocol::CommitSequence::new(50)
        )
    );
    assert_eq!(copy.as_of.acked_at.unix_millis(), NOW - 1);

    assert_eq!(
        settle(
            &fixture,
            durable_steps::Uuid::now_v7(),
            durable_steps::Uuid::now_v7(),
            old
        )
        .await?,
        json!({"kind":"older","value":{"stored":{"epoch":2,"incarnation":2,"change":1}}})
    );
    assert_eq!(directory(&fixture).await?, current);
    let projection: Value = serde_json::from_str(
        latest
            .1
            .as_deref()
            .ok_or("latest hook projection missing")?,
    )?;
    assert_eq!(
        settle(
            &fixture,
            durable_steps::Uuid::now_v7(),
            durable_steps::Uuid::now_v7(),
            projection
        )
        .await?,
        json!({"kind":"duplicate"})
    );
    assert_eq!(directory(&fixture).await?, current);
    assert_eq!(fixture.lease_and_notices().await?, latest);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 1_000).await?;
    assert_eq!(directory(&reopened).await?, current);
    assert_eq!(reopened.lease_and_notices().await?, latest);
    reopened.close().await
}
