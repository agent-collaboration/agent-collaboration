use super::*;

const COLUMNS: [&str; 9] = [
    "message_id",
    "sender",
    "message_kind",
    "state",
    "failure_reason",
    "cancel_due_at",
    "sender_request_id",
    "created_at",
    "updated_at",
];

fn value(text: &str) -> SqlValue {
    SqlValue::Text(text.to_owned())
}

fn stored(changes: &[(&str, SqlValue)]) -> ReadOutcome {
    let mut row = vec![
        value("01890f2e-7b4c-7cc0-98c4-0000000000a1"),
        value(r#"{"kind":"actor","value":"01890f2e-7b4c-7cc0-98c4-0000000000c1"}"#),
        value("direct"),
        value("queued"),
        SqlValue::Null,
        value("2100-01-01T00:20:00.000Z"),
        value("01890f2e-7b4c-7cc0-98c4-0000000000b1"),
        value("2100-01-01T00:01:00.000Z"),
        value("2100-01-01T00:01:00.000Z"),
    ];
    for (name, replacement) in changes {
        let index = COLUMNS.iter().position(|column| column == name).unwrap();
        row[index] = replacement.clone();
    }
    ReadOutcome {
        columns: COLUMNS.iter().map(|column| (*column).to_owned()).collect(),
        rows: vec![row],
    }
}

fn decode(rows: &ReadOutcome) -> Result<DirectMessageRow, StoreFailure> {
    decode_direct_message(rows, &rows.rows[0])
}

#[test]
fn a_queued_direct_message_decodes_with_its_mark_sender_and_request() {
    let row = decode(&stored(&[])).unwrap();
    assert_eq!(row.status, DirectMessageStatus::Queued);
    assert_eq!(
        row.cancel_due_at,
        "2100-01-01T00:20:00Z".parse::<Timestamp>().unwrap()
    );
    assert_eq!(
        row.sender_request_id.to_string(),
        "01890f2e-7b4c-7cc0-98c4-0000000000b1"
    );
    let failed = decode(&stored(&[
        ("state", value("failed")),
        ("failure_reason", value("recipientRefused")),
    ]))
    .unwrap();
    assert_eq!(
        failed.status,
        DirectMessageStatus::Failed(mail::MessageRefusal::RecipientRefused)
    );
    assert_eq!(
        refusal_text(mail::MessageRefusal::RecipientRefused).unwrap(),
        "recipientRefused"
    );
}

/// Only a direct message enters the reducer: an outcome message, any unknown
/// kind, a missing mark or request id, an off-grid mark, an unknown state or
/// refusal reason, and a central sender are refused, never coerced.
#[test]
fn every_invalid_stored_direct_message_is_refused() {
    for change in [
        vec![("message_kind", value("deliveryOutcome"))],
        vec![("message_kind", value("mystery"))],
        vec![("cancel_due_at", SqlValue::Null)],
        vec![("sender_request_id", SqlValue::Null)],
        vec![("cancel_due_at", value("2100-01-01T00:21:00.000Z"))],
        vec![("cancel_due_at", value("2100-01-01T00:00:00.000Z"))],
        vec![("state", value("sent"))],
        vec![("state", value("failed"))],
        vec![
            ("state", value("failed")),
            ("failure_reason", value("tooBusy")),
        ],
        vec![("failure_reason", value("recipientRefused"))],
        vec![("sender", value(r#"{"kind":"central"}"#))],
    ] {
        assert!(
            decode(&stored(&change)).is_err(),
            "accepted invalid row {change:?}"
        );
    }
}

fn identity(kind: &str, identity: &str) -> ReadOutcome {
    ReadOutcome {
        columns: vec!["identity_kind".to_owned(), "identity".to_owned()],
        rows: vec![vec![value(kind), value(identity)]],
    }
}

#[test]
fn an_agent_sender_has_a_session_to_tell_a_person_has_none_and_a_missing_actor_is_corrupt() {
    let session = r#"{"kind":"agentSession","session":{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000001","endpoint":"codex-local","sessionId":"sender"}}"#;
    let told = decode_sender_session(&identity("agentSession", session)).unwrap();
    assert_eq!(told.unwrap().session_id.as_str(), "sender");
    assert!(decode_sender_session(&identity("person", session)).is_err());
    let person = r#"{"kind":"person","personId":"01890f2e-7b4c-7cc0-98c4-0000000000d1"}"#;
    assert_eq!(
        decode_sender_session(&identity("person", person)).unwrap(),
        None
    );
    assert!(
        decode_sender_session(&ReadOutcome {
            columns: vec!["identity_kind".to_owned(), "identity".to_owned()],
            rows: vec![],
        })
        .is_err()
    );
}
