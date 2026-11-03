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
fn a_history_range_orders_positions_and_defers_epoch_lineage_to_the_snapshot() {
    let range = json!({"kind":"range",
        "from":{"kind":"project","cursor":{"epoch":1,"position":41}},
        "to":{"kind":"project","cursor":{"epoch":2,"position":42}}});
    assert!(serde_json::from_value::<PostSelection>(range).is_ok());
    let reversed = json!({"kind":"range",
        "from":{"kind":"project","cursor":{"epoch":1,"position":42}},
        "to":{"kind":"project","cursor":{"epoch":2,"position":41}}});
    assert!(serde_json::from_value::<PostSelection>(reversed).is_err());
    let project = "01890f2e-7b4c-7cc0-98c4-000000000001";
    let aggregate = json!({"kind":"range",
        "from":{"kind":"aggregate","cursor":{"sweep":null,"perProject":{
            project:{"epoch":1,"position":41}}}},
        "to":{"kind":"aggregate","cursor":{"sweep":null,"perProject":{
            project:{"epoch":2,"position":42}}}}});
    assert!(serde_json::from_value::<PostSelection>(aggregate).is_ok());
}

#[test]
fn latest_selection_rejects_unused_position_fields() {
    assert!(
        serde_json::from_value::<PostSelection>(json!({"kind":"latest","unused":true})).is_err()
    );
}
