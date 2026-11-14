use super::support::*;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Corrupt retained answers must fail through the real route without consuming proof or committing effects."
)]
#[tokio::test]
async fn corrupt_retained_answer_is_unavailable_without_any_replay_effect() -> Result {
    let fixture = Fixture::new().await?;
    fixture.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d7f3";
    let created = create(
        &fixture,
        &owner,
        operation,
        "Validated retained",
        "description",
    )
    .await?;
    assert_eq!(created.status, 200);
    let good = created.value()?;
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
        fixture
            .apply(
                A,
                "UPDATE request_log SET answer=? WHERE request_id=?",
                vec![text(encoded), text(operation)],
            )
            .await?;
        let before = all_effects(&fixture).await?;
        let reply = create(
            &fixture,
            &owner,
            operation,
            "Validated retained",
            "description",
        )
        .await?;
        assert_eq!(reply.status, 503);
        assert!(reply.body.is_empty());
        assert!(
            all_effects(&fixture).await? == before,
            "invalid retained answer consumed a proof or committed an effect"
        );
    }
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Verbatim HTTP replay must fail if the validated stored representation is re-encoded."
)]
#[tokio::test]
async fn validated_retained_answer_returns_its_original_json_text() -> Result {
    let fixture = Fixture::new().await?;
    fixture.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d7f1";
    let first = create(&fixture, &owner, operation, "Stored wire", "Retained text").await?;
    assert_eq!(first.status, 200);
    let retained = format!(
        "\n {{ \"workspace\" : {{ \"state\":\"active\", \"description\":\"Retained text\", \"name\":\"Stored wire\", \"workspaceId\":\"{operation}\" }}, \"result\" : \"applied\" }} \t"
    );
    let value: serde_json::Value = serde_json::from_str(&retained)?;
    assert_eq!(value, first.value()?);
    assert_ne!(retained.as_bytes(), first.body.as_slice());
    // Plant only another valid representation of an answer from a real Create.
    fixture
        .apply(
            A,
            "UPDATE request_log SET answer=? WHERE request_id=?",
            vec![text(&retained), text(operation)],
        )
        .await?;
    let before = durable_effects(&fixture).await?;
    let replay = create(&fixture, &owner, operation, "Stored wire", "Retained text").await?;
    assert_eq!(replay.status, 200);
    assert!(
        replay.body == retained.as_bytes(),
        "validated stored text was re-serialized instead of returned verbatim"
    );
    assert_eq!(replay.value()?, value);
    assert!(
        durable_effects(&fixture).await? == before,
        "replay rewrote a retained outcome or business row"
    );
    assert_eq!(
        fixture
            .rows(A, "SELECT COUNT(*) FROM organization_used_requests", vec![])
            .await?,
        vec![vec![number(2)?]]
    );
    assert_eq!(value["result"], json!("applied"));
    let mut extended = value;
    extended["workspace"]["futureViewField"] = json!({"revision":2});
    let extended_text = serde_json::to_string_pretty(&extended)?;
    fixture
        .apply(
            A,
            "UPDATE request_log SET answer=? WHERE request_id=?",
            vec![text(&extended_text), text(operation)],
        )
        .await?;
    let before = durable_effects(&fixture).await?;
    let replay = create(&fixture, &owner, operation, "Stored wire", "Retained text").await?;
    assert_eq!(replay.status, 200);
    assert!(
        replay.body == extended_text.as_bytes(),
        "an opaque future view field must not corrupt or re-encode a validated retained answer"
    );
    assert!(durable_effects(&fixture).await? == before);
    fixture.close().await
}
