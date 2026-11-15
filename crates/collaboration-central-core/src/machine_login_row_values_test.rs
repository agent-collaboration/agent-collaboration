use super::*;

#[test]
fn timestamps_accept_existing_rfc3339_forms_but_not_unrepresentable_precision() {
    assert_eq!(
        millis("2030-03-17T17:46:40Z", "created_at").unwrap(),
        1_900_000_000_000
    );
    assert_eq!(
        millis("2030-03-17T17:46:40.001+00:00", "updated_at").unwrap(),
        1_900_000_000_001
    );
    for malformed in [
        "1900000000",
        "1969-12-31T23:59:59Z",
        "2030-03-17T17:46:40.000001Z",
    ] {
        assert_eq!(millis(malformed, "created_at"), Err(corrupt("created_at")));
    }
    assert_eq!(
        seconds("2030-03-17T17:46:40.001Z", "code_expires_at"),
        Err(corrupt("code_expires_at"))
    );
}

#[test]
fn timestamp_writers_keep_text_units_and_refuse_overflow() {
    assert_eq!(
        timestamp(1_900_000_000_001, "updated_at").unwrap(),
        "2030-03-17T17:46:40.001Z"
    );
    assert_eq!(
        seconds_timestamp(UnixSeconds(1_900_000_000), "code_expires_at").unwrap(),
        "2030-03-17T17:46:40.000Z"
    );
    assert!(timestamp(-1, "created_at").is_err());
    assert!(seconds_timestamp(UnixSeconds(i64::MAX), "code_expires_at").is_err());
}

#[test]
fn private_identifiers_refuse_other_uuid_versions_and_noncanonical_text() {
    assert!(private_id("019a3dce-2000-7000-8000-000000000001", "machine_id").is_ok());
    for malformed in [
        "00000000-0000-4000-8000-000000000001",
        "019A3DCE-2000-7000-8000-000000000001",
        "not-an-id",
    ] {
        assert_eq!(
            private_id(malformed, "machine_id"),
            Err(corrupt("machine_id"))
        );
    }
}
