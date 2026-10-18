#![allow(clippy::expect_used)]

use collaboration_replication_protocol::{
    MachineFrame, Piece, ReadPage, SessionAnswer, SessionRefusal,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn assert_wire_round_trip<T: Serialize + DeserializeOwned>(wire: Value) {
    let decoded =
        serde_json::from_value::<T>(wire.clone()).expect("must decode current wire shape");
    assert_eq!(
        serde_json::to_value(decoded).expect("must encode current wire shape"),
        wire
    );
}

fn piece_wire() -> Value {
    json!({
        "project": "019f0000-0000-7000-8000-000000000001",
        "epoch": 1,
        "continuesFrom": {"epoch": 1, "commitId": 0},
        "through": {"epoch": 1, "commitId": 2},
        "transactions": [{"kind": "rows", "commit_id": 2, "rows": []}],
        "part": null,
        "summary": {"status": "open", "latestRecordAt": "2026-10-12T12:00:00Z"},
        "digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    })
}

#[test]
fn hello_carries_schema_support() {
    assert_wire_round_trip::<MachineFrame>(json!({
        "kind": "hello", "installation": "019f0000-0000-7000-8000-000000000002",
        "session_id": "019f0000-0000-7000-8000-000000000003",
        "schema_support": {"maxVersion": 7}
    }));
}

#[test]
fn piece_carries_summary_and_sender_digest() {
    assert_wire_round_trip::<Piece>(piece_wire());
}

#[test]
fn transaction_part_carries_assembly_identity_and_digest() {
    let mut wire = piece_wire();
    wire.as_object_mut().expect("piece object").insert(
        "part".into(),
        json!({
            "transaction": {"epoch": 1, "commitId": 2}, "index": 0, "count": 2,
            "transactionDigest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }),
    );
    wire.as_object_mut()
        .expect("piece object")
        .insert("summary".into(), Value::Null);
    assert_wire_round_trip::<Piece>(wire);
}

#[test]
fn staged_answer_leaves_acknowledged_position_unchanged() {
    assert_wire_round_trip::<SessionAnswer>(json!({"kind": "partStaged", "value": {
        "transaction": {"epoch": 1, "commitId": 2}, "index": 0,
        "acked": {"epoch": 1, "commitId": 0}
    }}));
}

#[test]
fn stale_epoch_carries_the_superseded_epochs_cutoff() {
    assert_wire_round_trip::<SessionRefusal>(json!({
        "kind": "staleEpoch", "current_epoch": 2,
        "epoch_ended_at": {"epoch": 1, "commitId": 17}
    }));
}

#[test]
fn snapshot_continuation_is_lossless_in_a_session_request() {
    assert_wire_round_trip::<MachineFrame>(json!({"kind": "request",
        "frame_id": "019f0000-0000-7000-8000-000000000005",
        "body": {"kind": "read", "value": {
            "scope": {"kind": "projectCopy", "project": "019f0000-0000-7000-8000-000000000001"},
            "after": {"kind": "snapshot", "value": {
                "position": {"epoch": 1, "commitId": 17}, "table": "records",
                "afterKey": [{"kind": "text", "value": "key"}]
            }}
        }}
    }));
}

#[test]
fn snapshot_page_names_project_position_and_schema() {
    assert_wire_round_trip::<ReadPage>(json!({"kind": "projectCopy", "value": {
        "kind": "page", "value": {
            "project": "019f0000-0000-7000-8000-000000000001",
            "position": {"epoch": 1, "commitId": 17},
            "schema": {"version": 7, "fingerprint": "schema-fingerprint"},
            "schemaSteps": [], "table": "records", "rows": [], "next": null,
            "asOf": "2026-10-12T12:00:00Z"
        }
    }}));
}

#[test]
fn moved_snapshot_names_the_new_position() {
    assert_wire_round_trip::<ReadPage>(json!({"kind": "projectCopy", "value": {
        "kind": "moved", "value": {"position": {"epoch": 2, "commitId": 3}}
    }}));
}
