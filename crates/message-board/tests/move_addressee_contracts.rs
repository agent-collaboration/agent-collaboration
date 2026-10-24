use message_board::{BoardOperation, RequestId};
use serde_json::json;

#[test]
fn moves_carry_typed_addressees_as_semantic_content() {
    let input = json!({"kind":"moveRequest","moveId":RequestId::generate(),"moveName":"advance","evidence":[],"reason":"ready","expectedRevision":0,"addressees":[{"kind":"actor","recipient":{"kind":"human","humanId":"owner"}}]});
    let parsed = serde_json::from_value::<BoardOperation>(input.clone());
    assert!(
        parsed.is_ok(),
        "move cannot express its addressee: {parsed:?}"
    );
    let mut invalid = input;
    invalid["addressees"][0]["recipient"] = json!({"kind":"unknown"});
    assert!(serde_json::from_value::<BoardOperation>(invalid).is_err());
}
