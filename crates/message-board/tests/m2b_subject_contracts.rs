//! Required subjects validate at the wire/domain boundary, including Unicode limits.
use message_board::*;
use serde_json::json;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Contract assertions; Result propagates fixture errors.
fn subject_lines_trim_and_accept_exactly_one_hundred_twenty_unicode_characters()
-> Result<(), Box<dyn std::error::Error>> {
    let line = SubjectLine::try_from(format!("  {}  ", "é".repeat(120)))?;
    assert_eq!(line.as_str(), "é".repeat(120));
    assert!(SubjectLine::try_from("é".repeat(121)).is_err());
    Ok(())
}
#[test]
fn subject_lines_reject_empty_and_multiline_values() {
    for invalid in ["", "  ", "one\ntwo", "one\rtwo"] {
        assert!(
            SubjectLine::try_from(invalid.to_owned()).is_err(),
            "{invalid:?}"
        );
        assert!(
            serde_json::from_value::<ThreadSubject>(
                json!({"kind":"project","subject":{"line":invalid}})
            )
            .is_err()
        );
    }
}
#[test]
fn required_subject_union_rejects_missing_unknown_and_extra_fields() {
    for invalid in [
        json!(null),
        json!({}),
        json!({"kind":"unknown","subject":"topic"}),
        json!({"kind":"project","subject":{"line":"topic","title":"copy"}}),
    ] {
        assert!(
            serde_json::from_value::<ThreadSubject>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}
