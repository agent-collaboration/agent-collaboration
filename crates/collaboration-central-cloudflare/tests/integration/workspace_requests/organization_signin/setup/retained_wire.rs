use super::{owned_rows, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};

#[tokio::test]
async fn corrupt_retained_answer_is_unavailable_without_any_replay_effect() {
    let fixture = ready().await;
    let owner = fixture.owner();
    let operation = operation(0x7f3);
    let (body, intent) = create(&operation, "Validated retained", "description");
    let first = fixture
        .post_bytes(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(first.0, 200);
    let good: Value = serde_json::from_slice(&first.1).unwrap();
    let mut invalid = vec![
        "not JSON".to_owned(),
        json!({"result":"records","records":[]}).to_string(),
        json!({"result":"refused","reason":"notPermitted"}).to_string(),
    ];
    for (field, value) in [
        ("workspaceId", json!("019f0000-0000-7000-8000-00000000d7f4")),
        ("name", json!("Other name")),
        ("description", json!("Other description")),
        ("state", json!("archived")),
    ] {
        let mut changed = good.clone();
        changed["workspace"][field] = value;
        invalid.push(changed.to_string());
    }
    for encoded in invalid {
        let applied = fixture
            .runtime()
            .await
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE request_log SET answer=? WHERE request_id=?",
                        args: vec![SqlValue::Text(encoded), SqlValue::Text(operation.clone())],
                    },
                    expect: RowExpectation::Exactly(1),
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap();
        assert!(matches!(applied, BatchOutcome::Applied { .. }));
        let before = owned_rows(&fixture).await;
        let reply = fixture
            .post_bytes(
                signed::CREATE,
                &body,
                &headers(&owner, &intent, NOW_SECONDS),
            )
            .await;
        assert_eq!(reply.0, 503);
        assert!(reply.1.is_empty());
        assert!(
            owned_rows(&fixture).await == before,
            "invalid retained answer consumed a proof or committed an effect"
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn validated_retained_answer_returns_its_original_json_text() {
    let fixture = ready().await;
    let owner = fixture.owner();
    let operation = operation(0x7f1);
    let (body, intent) = create(&operation, "Stored wire", "Retained text");
    let first = fixture
        .post_bytes(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(first.0, 200);
    let retained = format!(
        "\n {{ \"workspace\" : {{ \"state\":\"active\", \"description\":\"Retained text\", \"name\":\"Stored wire\", \"workspaceId\":\"{operation}\" }}, \"result\" : \"applied\" }} \t"
    );
    let value: Value = serde_json::from_str(&retained).unwrap();
    assert_eq!(value, serde_json::from_slice::<Value>(&first.1).unwrap());
    assert_ne!(retained.as_bytes(), first.1.as_slice());
    let applied = fixture
        .runtime()
        .await
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE request_log SET answer=? WHERE request_id=?",
                    args: vec![
                        SqlValue::Text(retained.clone()),
                        SqlValue::Text(operation.clone()),
                    ],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(applied, BatchOutcome::Applied { .. }));
    let before = stable_effects(&fixture).await;
    let replay = fixture
        .post_bytes(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(replay.0, 200);
    assert!(
        replay.1 == retained.as_bytes(),
        "validated stored text was re-serialized instead of returned verbatim"
    );
    assert_eq!(serde_json::from_slice::<Value>(&replay.1).unwrap(), value);
    assert!(
        stable_effects(&fixture).await == before,
        "replay rewrote a retained outcome or business row"
    );
    assert_eq!(
        fixture
            .read("SELECT COUNT(*) FROM organization_used_requests")
            .await
            .rows,
        vec![vec![number(2)]]
    );
    let mut extended = value;
    extended["workspace"]["futureViewField"] = json!({"revision":2});
    let extended_text = serde_json::to_string_pretty(&extended).unwrap();
    let applied = fixture
        .runtime()
        .await
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE request_log SET answer=? WHERE request_id=?",
                    args: vec![
                        SqlValue::Text(extended_text.clone()),
                        SqlValue::Text(operation.clone()),
                    ],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(applied, BatchOutcome::Applied { .. }));
    let before = stable_effects(&fixture).await;
    let replay = fixture
        .post_bytes(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(replay.0, 200);
    assert!(
        replay.1 == extended_text.as_bytes(),
        "an opaque future view field must not corrupt or re-encode a validated retained answer"
    );
    assert!(stable_effects(&fixture).await == before);
    fixture.close().await;
}
