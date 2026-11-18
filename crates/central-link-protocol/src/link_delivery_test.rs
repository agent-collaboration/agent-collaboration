use super::*;
use serde_json::json;

#[test]
fn delivery_vocabulary_has_its_closed_wire_names_and_send_ids_are_uuidv7() {
    assert_eq!(
        serde_json::to_value(DeliveryAnswer::RefusedForGood).unwrap(),
        json!("refusedForGood")
    );
    assert!(
        serde_json::from_value::<SendId>(json!("019f0000-0000-7000-8000-000000000001")).is_ok()
    );
    assert!(
        serde_json::from_value::<SendId>(json!("019f0000-0000-4000-8000-000000000001")).is_err()
    );
}
