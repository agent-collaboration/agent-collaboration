#![allow(clippy::panic_in_result_fn)]
use fleet_admission::*;
use message_board::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn wire_bundle() -> serde_json::Value {
    serde_json::json!({
        "provenance": {"installation":"550e8400-e29b-41d4-a716-446655440000","machine":"fixture machine"},
        "records":[
            {"kind":"workspace","id":"019f0000-0000-7000-8000-000000000001","name":"Workspace","description":"","repositories":[],"time":{"kind":"legacyUnknown"}},
            {"kind":"board","id":"019f0000-0000-7000-8000-000000000002","workspace":"019f0000-0000-7000-8000-000000000001","name":"Board","description":"","state":"active","time":{"kind":"legacyUnknown"}},
            {"kind":"directory","project":"019f0000-0000-7000-8000-000000000003","board":"019f0000-0000-7000-8000-000000000002","title":"Project","time":{"kind":"legacyUnknown"}}
        ]
    })
}

#[test]
fn bundle_without_declared_owner_is_refused_at_decode() {
    assert!(serde_json::from_value::<FleetImportBundle>(wire_bundle()).is_err());
}

fn bundle(owner: &str) -> TestResult<FleetImportBundle> {
    let mut wire = wire_bundle();
    wire.as_object_mut()
        .ok_or("fixture object missing")?
        .insert("declaredOwner".to_owned(), serde_json::json!(owner));
    Ok(serde_json::from_value(wire)?)
}

#[test]
fn every_imported_actor_is_the_declared_owner() -> TestResult {
    let decision = decide_import(&bundle("first-importer")?, &FleetSnapshot::empty())?;
    assert_eq!(decision.writes.len(), 3);
    let expected = RecordedActor::Human {
        human_id: HumanId::try_from("first-importer".to_owned())?,
    };
    for write in decision.writes {
        assert_eq!(write.record.envelope().actor, expected);
    }
    Ok(())
}

#[test]
fn different_declared_owner_replays_without_rewriting_first_attribution_or_digest() -> TestResult {
    let first = bundle("first-importer")?;
    let second = bundle("second-importer")?;
    assert_eq!(first.digest()?, second.digest()?);
    for (before, after) in first.records.iter().zip(&second.records) {
        assert_eq!(before.digest()?, after.digest()?);
    }
    let imported = decide_import(&first, &FleetSnapshot::empty())?;
    assert_eq!(
        imported.result,
        FleetImportResult::Imported {
            counts: FleetImportCounts {
                workspaces: 1,
                boards: 1,
                directory_entries: 1
            }
        }
    );
    let mut snapshot = FleetSnapshot::empty();
    for write in imported.writes {
        snapshot.imports.push(ImportedFleetRecord {
            record: write.record.reference(),
            digest: write.digest,
        });
        snapshot.records.push(write.record);
    }
    let stored = snapshot.records.clone();
    let replay = decide_import(&second, &snapshot)?;
    assert_eq!(replay.result, FleetImportResult::AlreadyImported);
    assert!(replay.writes.is_empty());
    assert!(replay.feed.is_empty());
    assert_eq!(snapshot.records, stored);
    let expected = RecordedActor::Human {
        human_id: HumanId::try_from("first-importer".to_owned())?,
    };
    for record in snapshot.records {
        assert_eq!(record.envelope().actor, expected);
    }
    Ok(())
}
