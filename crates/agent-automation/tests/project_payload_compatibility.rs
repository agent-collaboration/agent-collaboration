//! Optional project intent survives frozen/portable payloads; legacy values retain their exact JSON shape.
use agent_automation::{
    CapturedRunInputs, ExecutionDestination, PortableInstruction, PortableSchedulePackage,
    ScheduleDefinition, TimingRule, decode_schedule_package, encode_schedule_package,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

// No Default implementation: absence belongs to Option, not a fabricated project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
struct TestProject(String);
type Destination = ExecutionDestination<String, String, TestProject>;

#[test]
fn project_payload_legacy_absence_decodes_and_serializes_without_rewriting_the_value() {
    for legacy in [
        json!({"kind":"unprepared"}),
        json!({"kind":"freshEachRunUnprepared"}),
        json!({"kind":"ownedThread","target":"fixture-conversation","cwd":"/fixture"}),
        json!({"kind":"freshEachRun","endpoint":"fixture-endpoint","cwd":"/fixture"}),
    ] {
        let parsed: Destination = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), legacy);
    }
}

#[test]
fn project_payload_portable_copy_retains_project_while_removing_native_destination_bindings() {
    let original: Destination = serde_json::from_value(json!({"kind":"freshEachRun","endpoint":"machine-native-endpoint","cwd":"/machine/native/path","projectId":"fixture-project"})).unwrap();
    let instruction_id: agent_automation::InstructionId = "019f0000-0000-7000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap();
    let package = PortableSchedulePackage::<String, String, TestProject> {
        schedule_id: "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        source_change_id: "fixture-change".to_owned(),
        instruction: PortableInstruction {
            instruction_id: instruction_id.clone(),
            text: "Fixture instructions".to_owned().try_into().unwrap(),
            source_revision_id: "fixture-revision".to_owned(),
        },
        definition: ScheduleDefinition {
            instruction_id,
            timing: TimingRule::Interval { seconds: 600 },
            enabled: true,
            destination: original,
            execution_timeout_seconds: None,
            model: None,
            effort: None,
        },
        continuity: None,
    };
    let before = serde_json::to_value(&package.definition).unwrap();
    let encoded = encode_schedule_package(&package, 1_048_576).unwrap();
    let decoded: PortableSchedulePackage<String, String, TestProject> =
        decode_schedule_package(&encoded, 1_048_576).unwrap();
    let portable = decoded.definition.destination;

    assert_eq!(
        serde_json::to_value(&portable).unwrap(),
        json!({"kind":"freshEachRunUnprepared","projectId":"fixture-project"})
    );
    assert_eq!(serde_json::to_value(&package.definition).unwrap(), before);
    let restored: Destination =
        serde_json::from_value(serde_json::to_value(portable).unwrap()).unwrap();
    assert!(!restored.is_prepared());
}

#[test]
fn project_payload_frozen_inputs_keep_the_named_project_through_serialization() {
    let wire = json!({
        "scheduleChangeId":"019f0000-0000-7000-8000-000000000001",
        "instructionRevisionId":"019f0000-0000-7000-8000-000000000002",
        "instructionText":"Fixture instructions", "continuity":{"kind":"none"},
        "executionConfiguration":{"destination":{"kind":"freshEachRun","endpoint":"fixture-endpoint","cwd":"/fixture","projectId":"fixture-project"},"executionTimeoutSeconds":null,"model":null,"effort":null}
    });
    let frozen: CapturedRunInputs<String, String, TestProject> =
        serde_json::from_value(wire.clone()).unwrap();

    assert_eq!(serde_json::to_value(frozen).unwrap(), wire);
}
