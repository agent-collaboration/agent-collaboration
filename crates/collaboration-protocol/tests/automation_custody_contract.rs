use collaboration_protocol::{
    BatchEntries, FiringOutcome, FiringWork, MachineWorkProtocol, PullAutomationBatches,
    ReportWorkItems, WorkEntry, WorkItemReport,
};
use serde_json::{Value, json};

fn firing_entry() -> Value {
    json!({"kind":"firing","value":{
        "firingId":"019f0000-0000-7000-8000-000000000001",
        "automationId":"019f0000-0000-7000-8000-000000000002",
        "dueAt":300000,
        "revision":"019f0000-0000-7000-8000-000000000003",
        "content":{"contentId":"019f0000-0000-7000-8000-000000000004","revisionId":"019f0000-0000-7000-8000-000000000006","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "work":{"kind":"wake","createdBy":{"kind":"owner"},"timing":{"kind":"at","at":"1970-01-01T00:05:00Z"},"anchorAt":"1970-01-01T00:00:00Z","createdAt":"1970-01-01T00:00:00Z","target":{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000005","endpointId":"claude-local"},"sessionId":"test"},"origin":{"kind":"router"},"delivery":"auto","generationGuard":null,"expiresAt":"1970-01-01T00:07:00Z"}
    }})
}

#[test]
fn pull_capabilities_are_explicit_typed_and_replaceable() {
    let schema = serde_json::to_value(schemars::schema_for!(PullAutomationBatches)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let pull = json!({"after":null,"maxBytes":65536,"workProtocols":["machineWorkV1"]});
    assert!(validator.is_valid(&pull));
    let decoded: PullAutomationBatches = serde_json::from_value(pull.clone()).unwrap();
    assert_eq!(
        decoded.work_protocols,
        vec![MachineWorkProtocol::MachineWorkV1]
    );
    assert_eq!(serde_json::to_value(decoded).unwrap(), pull);

    let withdrawn = json!({"after":null,"maxBytes":65536,"workProtocols":[]});
    assert!(validator.is_valid(&withdrawn));
    assert!(
        serde_json::from_value::<PullAutomationBatches>(withdrawn)
            .unwrap()
            .work_protocols
            .is_empty()
    );

    for corrupt in [
        json!({"after":null,"maxBytes":65536}),
        json!({"after":null,"maxBytes":65536,"workProtocols":["machineWorkV2"]}),
    ] {
        assert!(!validator.is_valid(&corrupt));
        assert!(serde_json::from_value::<PullAutomationBatches>(corrupt).is_err());
    }
}

#[test]
fn batch_entry_limit_is_enforced_by_both_schema_and_stored_decode() {
    let entry = firing_entry();
    let schema = serde_json::to_value(schemars::schema_for!(BatchEntries)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for (count, accepted) in [(0, false), (1, true), (100, true), (101, false)] {
        let stored = Value::Array(vec![entry.clone(); count]);
        assert_eq!(
            validator.is_valid(&stored),
            accepted,
            "schema count {count}"
        );
        let decoded = serde_json::from_value::<BatchEntries>(stored);
        assert_eq!(
            decoded.is_ok(),
            accepted,
            "decode count {count}: {decoded:?}"
        );
        if !accepted {
            assert!(decoded.unwrap_err().to_string().contains("limitExceeded"));
        }
    }
}

#[test]
fn firing_content_is_a_reference_and_expiry_is_not_forced_onto_the_grid() {
    let entry = firing_entry();
    let decoded: WorkEntry = serde_json::from_value(entry.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), entry);
    let mut missing_header = entry.clone();
    missing_header["value"]["work"]
        .as_object_mut()
        .unwrap()
        .remove("createdBy");
    assert!(serde_json::from_value::<WorkEntry>(missing_header).is_err());
    let mut corrupt_repeat = entry.clone();
    corrupt_repeat["value"]["work"]["timing"] = json!({"kind":"interval","seconds":300});
    assert!(
        serde_json::from_value::<WorkEntry>(corrupt_repeat)
            .unwrap_err()
            .to_string()
            .contains("WAKE_MIN_PERIOD")
    );
    let mut corrupt = entry.clone();
    corrupt["value"]["content"]["text"] = json!("Inline content is forbidden");
    assert!(serde_json::from_value::<WorkEntry>(corrupt).is_err());
    let mut corrupt = entry;
    corrupt["value"]["dueAt"] = json!(120000);
    assert!(serde_json::from_value::<WorkEntry>(corrupt).is_err());
}

#[test]
fn schedule_firing_freezes_the_definition_needed_for_projection_and_run_inputs() {
    let mut entry = firing_entry();
    let target = entry["value"]["work"]["target"].clone();
    entry["value"]["work"] = json!({"kind":"schedule","definition":{
        "instructionId":"019f0000-0000-7000-8000-000000000007",
        "timing":{"kind":"interval","seconds":300,"firstDue":"1970-01-01T00:05:00Z"},
        "enabled":true,
        "destination":{"kind":"ownedThread","target":target,"cwd":"/fixture"},
        "executionTimeoutSeconds":600,"model":"claude-haiku-5-5","effort":null
    }});
    let schema = serde_json::to_value(schemars::schema_for!(WorkEntry)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(
        validator.is_valid(&entry),
        "{:#?}",
        validator.validate(&entry)
    );
    let decoded: WorkEntry = serde_json::from_value(entry.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), entry);
    let WorkEntry::Firing(firing) = decoded else {
        panic!("expected firing");
    };
    let FiringWork::Schedule { definition } = firing.work else {
        panic!("expected schedule definition");
    };
    assert_eq!(
        definition.instruction_id.as_str(),
        "019f0000-0000-7000-8000-000000000007"
    );
    assert_eq!(definition.model.as_deref(), Some("claude-haiku-5-5"));
    assert_eq!(
        firing.revision.as_str(),
        "019f0000-0000-7000-8000-000000000003"
    );
    assert_eq!(
        firing.content.revision_id.as_str(),
        "019f0000-0000-7000-8000-000000000006"
    );

    for invalid in [
        json!({"kind":"after","seconds":300}),
        json!({"kind":"interval","seconds":60}),
    ] {
        let mut corrupt = entry.clone();
        corrupt["value"]["work"]["definition"]["timing"] = invalid;
        assert!(!validator.is_valid(&corrupt));
        assert!(
            serde_json::from_value::<WorkEntry>(corrupt)
                .unwrap_err()
                .to_string()
                .contains("offGrid")
        );
    }
    entry["value"]["work"] = json!({"kind":"schedule","execution":{}});
    assert!(serde_json::from_value::<WorkEntry>(entry).is_err());
}

#[test]
fn report_envelope_and_durable_sequence_reject_corrupt_values() {
    let report = json!({"kind":"firing","value":{
        "firingId":"019f0000-0000-7000-8000-000000000001", "seq":1,
        "outcome":{"state":"delivered"}}});
    for (count, accepted) in [(0, false), (1, true), (100, true), (101, false)] {
        let value = json!({"reports":vec![report.clone();count]});
        assert_eq!(
            serde_json::from_value::<ReportWorkItems>(value).is_ok(),
            accepted
        );
    }
    let mut corrupt = report;
    corrupt["value"]["seq"] = json!(0);
    assert!(serde_json::from_value::<WorkItemReport>(corrupt).is_err());
    assert!(FiringOutcome::Delivered.is_terminal());
    assert!(
        !FiringOutcome::Uncertain {
            reason: "receipt lost".to_owned().try_into().unwrap()
        }
        .is_terminal()
    );
}

#[test]
fn tagged_unit_outcomes_reject_unknown_fields_without_changing_valid_wire()
-> Result<(), Box<dyn std::error::Error>> {
    use collaboration_protocol::{
        FiringMessageOrigin, FirstInputState, LocalRunOutcome, RemoteCreateRefusal,
        SessionCreateState,
    };
    println!(
        "AUTOMATION_TAGGED_UNIT_SCHEMAS {}",
        json!({
            "FiringMessageOrigin":schemars::schema_for!(FiringMessageOrigin),
            "FiringOutcome":schemars::schema_for!(FiringOutcome),
            "SessionCreateState":schemars::schema_for!(SessionCreateState),
            "RemoteCreateRefusal":schemars::schema_for!(RemoteCreateRefusal),
            "FirstInputState":schemars::schema_for!(FirstInputState),
            "LocalRunOutcome":schemars::schema_for!(LocalRunOutcome),
        })
    );
    strict_unit_variants::<FiringMessageOrigin>("kind", &["humanUser", "router"])?;
    strict_unit_variants::<FiringOutcome>(
        "state",
        &[
            "pending",
            "cancelled",
            "held",
            "received",
            "delivered",
            "expired",
            "unknown",
        ],
    )?;
    strict_unit_variants::<SessionCreateState>("state", &["unknown"])?;
    strict_unit_variants::<RemoteCreateRefusal>(
        "kind",
        &[
            "originMismatch",
            "creatorNotSupported",
            "approverRequired",
            "approverNotSupported",
        ],
    )?;
    strict_unit_variants::<FirstInputState>("state", &["none", "accepted", "unknown"])?;
    strict_unit_variants::<LocalRunOutcome>("state", &["finished"])?;
    Ok(())
}

fn strict_unit_variants<Outcome>(
    tag: &str,
    variants: &[&str],
) -> Result<(), Box<dyn std::error::Error>>
where
    Outcome: serde::de::DeserializeOwned + serde::Serialize + schemars::JsonSchema,
{
    let schema = serde_json::to_value(schemars::schema_for!(Outcome))?;
    let validator = jsonschema::validator_for(&schema)?;
    for variant in variants {
        let valid = json!({tag:variant});
        if !validator.is_valid(&valid) {
            return Err(format!("valid unit variant rejected: {valid}").into());
        }
        let decoded: Outcome = serde_json::from_value(valid.clone())?;
        if serde_json::to_value(decoded)? != valid {
            return Err(format!("valid unit variant changed on roundtrip: {valid}").into());
        }
        let corrupt = json!({tag:variant,"unexpected":true});
        if validator.is_valid(&corrupt) {
            return Err(format!("schema permits unknown field: {corrupt}").into());
        }
        if serde_json::from_value::<Outcome>(corrupt.clone()).is_ok() {
            return Err(format!("decoder silently drops unknown field: {corrupt}").into());
        }
    }
    Ok(())
}
