#![allow(clippy::unwrap_used)]
use fleet_protocol::{ClientFrame, ServerFrame};
use serde_json::json;

const SEND_ID: &str = "019eef00-0000-7000-8000-000000000001";

#[test]
fn new_batch_and_acknowledgment_vocabulary_decodes_in_both_directions() {
    for kind in ["requestBatch", "answerBatch"] {
        let wire = json!({"kind":kind,"value":[{
            "sendId":SEND_ID,"body":{"kind":"consumer","value":null}
        }]});
        assert!(
            serde_json::from_value::<ClientFrame>(wire.clone()).is_ok(),
            "{wire}"
        );
        assert!(
            serde_json::from_value::<ServerFrame>(wire.clone()).is_ok(),
            "{wire}"
        );
    }
    for kind in ["request", "answer"] {
        let wire = json!({"kind":kind,"value":{
            "frame":SEND_ID,"body":{"kind":"noticesApplied","value":{"through":40}}
        }});
        if kind == "request" {
            assert!(
                serde_json::from_value::<ClientFrame>(wire.clone()).is_ok(),
                "{wire}"
            );
        } else {
            assert!(
                serde_json::from_value::<ServerFrame>(wire.clone()).is_ok(),
                "{wire}"
            );
        }
    }
}

fn request_batch(bodies: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"kind":"requestBatch","value":bodies.into_iter().enumerate().map(|(index,body)| {
        json!({"sendId":format!("019eef00-0000-7000-8000-{:012x}",index+1),"body":body})
    }).collect::<Vec<_>>()})
}

#[test]
fn batches_refuse_mixed_operation_kinds_and_invalid_identifiers() {
    let claim = json!({"kind":"lease","value":{"kind":"claim","value":{
        "attempt":SEND_ID,"database":SEND_ID
    }}});
    let notice = json!({"kind":"noticesApplied","value":{"through":40}});
    assert!(
        serde_json::from_value::<ClientFrame>(request_batch(vec![claim.clone(), notice.clone()]))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ClientFrame>(request_batch(vec![notice.clone(), notice])).is_ok()
    );
    let mut wire = request_batch(vec![claim]);
    wire["value"][0]["sendId"] = json!("019eef00-0000-4000-8000-000000000001");
    assert!(serde_json::from_value::<ClientFrame>(wire).is_err());
}

#[test]
fn batch_count_limit_and_unknown_fields_are_enforced() {
    let body = json!({"kind":"consumer","value":null});
    for count in [0, 33] {
        assert!(
            serde_json::from_value::<ClientFrame>(request_batch(vec![body.clone(); count]))
                .is_err()
        );
    }
    assert!(serde_json::from_value::<ClientFrame>(request_batch(vec![body.clone(); 32])).is_ok());
    let mut item_extra = request_batch(vec![body.clone()]);
    item_extra["value"][0]["extra"] = json!(true);
    assert!(serde_json::from_value::<ClientFrame>(item_extra).is_err());
    let mut frame_extra = request_batch(vec![body]);
    frame_extra["extra"] = json!(true);
    assert!(serde_json::from_value::<ClientFrame>(frame_extra).is_err());
}

#[test]
fn batch_byte_limit_includes_envelope_and_single_frames_keep_large_items() {
    use fleet_protocol::{ConsumerProtocol, NoDelivery};
    struct TextProtocol;
    impl ConsumerProtocol for TextProtocol {
        type Delivery = NoDelivery;
        type Request = String;
        type Answer = String;
        type ServerRequest = String;
        type ClientAnswer = String;
        type Notice = ();
        type Annotation = ();
    }
    let empty = request_batch(vec![json!({"kind":"consumer","value":""})]);
    let overhead = serde_json::to_vec(&empty).unwrap().len();
    let mut exact = empty;
    exact["value"][0]["body"]["value"] = json!("a".repeat(1024 * 1024 - overhead));
    assert_eq!(serde_json::to_vec(&exact).unwrap().len(), 1024 * 1024);
    assert!(serde_json::from_value::<ClientFrame<TextProtocol>>(exact.clone()).is_ok());
    exact["value"][0]["body"]["value"] = json!("a".repeat(1024 * 1024 - overhead + 1));
    assert!(serde_json::from_value::<ClientFrame<TextProtocol>>(exact).is_err());
    let single = json!({"kind":"request","value":{"frame":SEND_ID,"body":{
        "kind":"consumer","value":"a".repeat(2*1024*1024)
    }}});
    assert!(serde_json::from_value::<ClientFrame<TextProtocol>>(single).is_ok());
}

#[test]
fn answer_batches_preserve_order_ids_and_independent_outcomes() {
    let wire = json!({"kind":"answerBatch","value":[
        {"sendId":SEND_ID,"body":{"kind":"consumer","value":null}},
        {"sendId":"019eef00-0000-7000-8000-000000000002","body":{"kind":"invalidShape","value":{"field":"body"}}}
    ]});
    let decoded: ServerFrame = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
}

#[test]
fn batches_refuse_different_operations_inside_the_same_outer_family() {
    let call = json!({"operation":SEND_ID,"step":"wire"});
    let fetch = json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"call":call,"after":0,"limit":1}}});
    let ack =
        json!({"kind":"delivery","value":{"kind":"statusAck","value":{"call":call,"through":0}}});
    assert!(
        serde_json::from_value::<ClientFrame>(request_batch(vec![fetch.clone(), fetch])).is_ok()
    );
    assert!(serde_json::from_value::<ClientFrame>(request_batch(vec![ack, json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"call":call,"after":0,"limit":1}}})])).is_err());
}
