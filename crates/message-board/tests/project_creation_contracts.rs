use message_board::*;
use serde_json::json;
mod support;

#[test]
fn project_creation_fact_retains_domain_input_for_the_original_directory_request() {
    let project = ProjectId::generate();
    let mut envelope = support::record_envelope(project.clone().into(), project.clone());
    envelope.scope.project = None;
    envelope.position = RecordPlacement::Fleet(FleetFeedEntry {
        position: FleetPosition::ZERO,
        origin: support::actor_origin(),
    });
    let entry = ProjectDirectoryEntry {
        envelope,
        id: project,
        board: BoardId::generate(),
        title: ResourceName::try_from("public bootstrap".to_owned()).unwrap(),
    };
    let value = json!({"kind":"projectCreated","entry":entry,
        "creatingRequest":RequestId::generate(),"actingFor":{"kind":"human","humanId":"owner"}});
    let decoded = serde_json::from_value::<LifecycleFact>(value.clone());
    assert!(decoded.is_ok(), "durable domain input: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), value);
    let mut unknown = value;
    unknown["apiRequest"] = json!({});
    assert!(serde_json::from_value::<LifecycleFact>(unknown).is_err());
}
