#![allow(clippy::expect_used)]

use collaboration_replication_protocol::{
    CommitId, Epoch, Piece, PieceContents, PieceDigest, ReplicatedTransaction, RowChange, SqlValue,
    TransactionKey, TransactionPart,
};
use serde_json::{Value, json};

fn content_wire() -> Value {
    json!({
        "project": "019f0000-0000-7000-8000-000000000001", "epoch": 1,
        "continuesFrom": {"epoch": 1, "commitId": 0}, "through": {"epoch": 1, "commitId": 2},
        "transactions": [{"kind": "rows", "commit_id": 2, "rows": []}], "part": null,
        "summary": {"status": "open", "latestRecordAt": "2026-10-12T12:00:00Z"}
    })
}

#[test]
fn canonical_digest_matches_independent_sha256_golden() {
    // Python hashlib.sha256 of the literal canonical typed tuple:
    // ["019f0000-0000-7000-8000-000000000001",1,{"epoch":1,"commitId":0},{"epoch":1,"commitId":2},[{"kind":"rows","commit_id":2,"rows":[]}],null,{"status":"open","latestRecordAt":"2026-10-12T12:00:00Z"}]
    let contents: PieceContents = serde_json::from_value(content_wire()).expect("valid contents");
    let piece = Piece::new(contents).expect("valid piece");
    assert_eq!(
        piece.digest.as_str(),
        "2baeb5a8d4d93161f623a50e9da2e478c655c28e7f72a974dc9e30a249a88d65"
    );
    assert_eq!(piece.recomputed_digest().expect("digest"), piece.digest);
}

#[test]
fn decoding_preserves_sender_digest_for_copy_conflict_decision() {
    let mut wire = content_wire();
    wire.as_object_mut().expect("object").insert(
        "digest".into(),
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    );
    let piece: Piece = serde_json::from_value(wire).expect("valid envelope with mismatched digest");
    assert_eq!(
        piece.digest.as_str(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_ne!(piece.recomputed_digest().expect("digest"), piece.digest);
}

#[test]
fn transport_object_order_and_whitespace_do_not_change_digest() {
    let contents: PieceContents = serde_json::from_str(
        r#"{
        "summary": {"latestRecordAt":"2026-10-12T12:00:00Z","status":"open"},
        "part":null,"through":{"commitId":2,"epoch":1},
        "transactions":[{"rows":[],"commit_id":2,"kind":"rows"}],
        "continuesFrom":{"commitId":0,"epoch":1},"epoch":1,
        "project":"019f0000-0000-7000-8000-000000000001"
    }"#,
    )
    .expect("valid reordered contents");
    assert_eq!(
        Piece::new(contents).expect("piece").digest.as_str(),
        "2baeb5a8d4d93161f623a50e9da2e478c655c28e7f72a974dc9e30a249a88d65"
    );
}

#[test]
fn every_content_field_is_covered_by_digest() {
    let contents: PieceContents = serde_json::from_value(content_wire()).expect("contents");
    let original = Piece::new(contents.clone()).expect("piece");
    let mut status_changed = contents;
    status_changed.summary.as_mut().expect("summary").status = "closed".into();
    assert_ne!(
        Piece::new(status_changed).expect("piece").digest,
        original.digest
    );
    let mut modified = serde_json::to_value(&original).expect("encode");
    modified.as_object_mut().expect("object").insert(
        "project".into(),
        json!("019f0000-0000-7000-8000-000000000002"),
    );
    let changed: Piece = serde_json::from_value(modified).expect("changed project");
    assert_ne!(
        changed.recomputed_digest().expect("digest"),
        original.digest
    );
    let mut variants = Vec::new();
    let mut changed = original.clone();
    changed.epoch = Epoch::new(2).expect("epoch");
    variants.push(changed);
    let mut changed = original.clone();
    changed.continues_from.commit_id = CommitId::new(1);
    variants.push(changed);
    let mut changed = original.clone();
    changed.through.commit_id = CommitId::new(3);
    variants.push(changed);
    let mut changed = original.clone();
    changed.transactions = vec![ReplicatedTransaction::Rows {
        commit_id: CommitId::new(2),
        rows: vec![RowChange::delete("records", vec![SqlValue::Integer(1)]).expect("row")],
    }];
    variants.push(changed);
    let mut changed = original.clone();
    changed.part = Some(
        TransactionPart::new(
            TransactionKey {
                epoch: original.epoch,
                commit_id: original.through.commit_id,
            },
            0,
            1,
            PieceDigest::of_transaction(original.transactions.first().expect("transaction"))
                .expect("digest"),
        )
        .expect("part"),
    );
    variants.push(changed);
    for changed in variants {
        assert_ne!(
            changed.recomputed_digest().expect("digest"),
            original.digest
        );
    }
    let mut changed = original.clone();
    changed.digest = PieceDigest::try_from(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
    )
    .expect("digest");
    assert_eq!(
        changed.recomputed_digest().expect("digest"),
        original.digest
    );
}

#[test]
fn integer_precision_survives_hashing_and_sql_value_round_trip() {
    for value in [
        i64::MIN,
        i64::MAX,
        9_007_199_254_740_992,
        9_007_199_254_740_993,
    ] {
        let sql_value = SqlValue::Integer(value);
        let encoded = serde_json::to_vec(&sql_value).expect("encode integer");
        assert_eq!(
            serde_json::from_slice::<SqlValue>(&encoded).expect("decode"),
            sql_value
        );
    }
    let make_transaction = |value: i64| {
        serde_json::from_value::<ReplicatedTransaction>(json!({
        "kind": "rows", "commit_id": 2, "rows": [{"kind": "upsert", "table": "records",
            "key": [{"kind": "integer", "value": 1}], "values": [{"kind": "integer", "value": value}]}]
    })).expect("transaction")
    };
    let first =
        PieceDigest::of_transaction(&make_transaction(9_007_199_254_740_992)).expect("digest");
    let second =
        PieceDigest::of_transaction(&make_transaction(9_007_199_254_740_993)).expect("digest");
    assert_ne!(first, second);
}

#[test]
fn digest_encoding_rejects_invalid_hex_and_nonfinite_real_values() {
    for digest in [
        "",
        "a",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
    ] {
        assert!(serde_json::from_value::<PieceDigest>(json!(digest)).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(serde_json::to_vec(&SqlValue::Real(value)).is_err());
    }
}

#[test]
fn piece_shape_rejects_inconsistent_epoch_part_and_summary() {
    let base = content_wire();
    let mutations = [
        ("epoch", json!(2)),
        ("through", json!({"epoch": 2, "commitId": 2})),
        ("summary", Value::Null),
        (
            "summary",
            json!({"status": "", "latestRecordAt": "2026-10-12T12:00:00Z"}),
        ),
        (
            "part",
            json!({"transaction": {"epoch": 1, "commitId": 3}, "index": 1, "count": 2,
            "transactionDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
        ),
        (
            "part",
            json!({"transaction": {"epoch": 1, "commitId": 2}, "index": 0, "count": 2,
            "transactionDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
        ),
    ];
    for (field, value) in mutations {
        let mut mutated = base.clone();
        mutated
            .as_object_mut()
            .expect("object")
            .insert(field.into(), value);
        let contents: PieceContents = serde_json::from_value(mutated).expect("shape");
        assert!(Piece::new(contents).is_err(), "must reject {field}");
    }
}
