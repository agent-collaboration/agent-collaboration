use message_board::*;
mod support;

#[test]
fn project_state_round_trip_contains_only_project_lifecycle_fields() {
    let project = ProjectId::generate();
    let expected = serde_json::json!({
        "envelope": support::record_envelope(project.clone().into(), project.clone()),
        "projectId": project,
        "description": "Project lifecycle",
        "revision": 1,
        "closedAt": null
    });
    let state: ProjectState = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(state).unwrap(), expected);
}
