use message_board::{Post, PostSelection};
use serde_json::json;

#[test]
fn project_history_selection_requires_an_epoch_qualified_cursor() {
    let current = json!({"kind":"afterPosition", "after":{"kind":"project",
        "cursor":{"epoch":1,"position":42}}});
    let decoded = serde_json::from_value::<PostSelection>(current);
    assert!(decoded.is_ok(), "project cursor: {decoded:?}");
    assert!(
        serde_json::from_value::<PostSelection>(json!({"kind":"afterPosition",
        "afterActivitySequence":42}))
        .is_err()
    );
}

#[test]
fn aggregate_history_selection_preserves_independent_project_positions() {
    let current = json!({"kind":"afterPosition", "after":{"kind":"aggregate",
    "cursor":{"sweep":null,"perProject":{
        "01890f2e-7b4c-7cc0-98c4-000000000001":{"epoch":1,"position":41},
        "01890f2e-7b4c-7cc0-98c4-000000000002":{"epoch":3,"position":2}
    }}}});
    let decoded = serde_json::from_value::<PostSelection>(current.clone());
    assert!(decoded.is_ok(), "aggregate cursor: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), current);
}

#[test]
fn post_schema_replaces_the_global_scalar_with_a_record_position() {
    let schema = serde_json::to_value(schemars::schema_for!(Post)).unwrap();
    let properties = schema["properties"].as_object().unwrap();
    assert!(properties.contains_key("position"));
    assert!(!properties.contains_key("activitySequence"));
}

#[test]
fn a_history_range_cannot_compare_positions_from_different_writer_epochs() {
    let range = json!({"kind":"range",
        "from":{"kind":"project","cursor":{"epoch":1,"position":41}},
        "to":{"kind":"project","cursor":{"epoch":2,"position":42}}});
    assert!(serde_json::from_value::<PostSelection>(range).is_err());
}

#[test]
fn latest_selection_rejects_unused_position_fields() {
    assert!(
        serde_json::from_value::<PostSelection>(json!({"kind":"latest","unused":true})).is_err()
    );
}
