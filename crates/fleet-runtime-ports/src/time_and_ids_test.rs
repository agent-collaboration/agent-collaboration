use super::{SyncTime, canonical_uuid_v7, uuid_v7};
use std::time::Duration;
use uuid::Uuid;

#[test]
fn only_version_seven_uuids_pass() {
    let v7 = Uuid::parse_str("01a1edaf-6053-719b-9624-24ea745c2092").unwrap();
    let v4 = Uuid::parse_str("3f0e6b8c-1c9d-4a5e-8f2b-6d7c8e9fa0b1").unwrap();
    assert!(uuid_v7(v7));
    assert!(!uuid_v7(v4));
}

#[test]
fn canonical_text_is_lowercase_hyphenated_v7_only() {
    assert!(canonical_uuid_v7("01a1edaf-6053-719b-9624-24ea745c2092"));
    assert!(!canonical_uuid_v7("01A1EDAF-6053-719B-9624-24EA745C2092"));
    assert!(!canonical_uuid_v7("01a1edaf6053719b962424ea745c2092"));
    assert!(!canonical_uuid_v7("3f0e6b8c-1c9d-4a5e-8f2b-6d7c8e9fa0b1"));
}

#[test]
fn a_deadline_saturates_instead_of_wrapping() {
    let late = SyncTime::from_unix_millis(i64::MAX - 5);
    assert_eq!(late.after(Duration::from_secs(1)).unix_millis(), i64::MAX);
    assert_eq!(
        SyncTime::from_unix_millis(1_000).after(Duration::from_millis(250)),
        SyncTime::from_unix_millis(1_250)
    );
}
