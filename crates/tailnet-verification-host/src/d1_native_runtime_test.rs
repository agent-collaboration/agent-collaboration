use super::*;

#[test]
fn backend_publication_uses_explicit_utc_z_instead_of_an_offset() {
    let time = publication_time();
    assert!(time.ends_with('Z'));
    assert!(!time.ends_with("+00:00"));
    assert!(chrono::DateTime::parse_from_rfc3339(&time).is_ok());
}
