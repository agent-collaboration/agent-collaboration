//! Copy provenance and writer authority have distinct, strict public wire states.
use message_board::{CopySource, CurrentWriter, EpochNumber};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Assertions compare the wire contract; Result propagates fixture errors.
fn copy_source_only_exposes_a_writer_or_an_acknowledged_copy() -> TestResult {
    for (source, expected) in [
        (CopySource::Writer {}, json!({"kind":"writer"})),
        (CopySource::ReadCopy {}, json!({"kind":"readCopy"})),
    ] {
        let encoded = serde_json::to_value(source)?;
        assert_eq!(encoded, expected);
        assert_eq!(serde_json::from_value::<CopySource>(encoded)?, source);
    }
    for invalid in [
        json!("writer"),
        json!("readCopy"),
        json!({"kind":"writer","supersededBy":3}),
        json!({"kind":"superseded","supersededBy":3}),
        json!({"kind":"superseded","supersededBy":null}),
        json!({"kind":"superseded","supersededBy":0}),
        json!({"kind":"superseded","supersededBy":3,"future":true}),
    ] {
        assert!(serde_json::from_value::<CopySource>(invalid).is_err());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Assertions compare the wire contract; Result propagates fixture errors.
fn current_writer_keeps_a_grant_unheld_import_and_unknown_distinct() -> TestResult {
    for (writer, expected) in [
        (
            CurrentWriter::Held {
                epoch: EpochNumber::try_from(4)?,
            },
            json!({"kind":"held","epoch":4}),
        ),
        (CurrentWriter::Unheld {}, json!({"kind":"unheld"})),
        (CurrentWriter::Unknown {}, json!({"kind":"unknown"})),
    ] {
        let encoded = serde_json::to_value(writer)?;
        assert_eq!(encoded, expected);
        assert_eq!(serde_json::from_value::<CurrentWriter>(encoded)?, writer);
    }
    for invalid in [
        json!(1),
        json!(null),
        json!({"kind":"held"}),
        json!({"kind":"held","epoch":0}),
        json!({"kind":"provisional"}),
        json!({"kind":"provisional","epoch":1}),
        json!({"kind":"unheld","epoch":2}),
        json!({"kind":"unknown","epoch":3}),
        json!({"kind":"future"}),
    ] {
        assert!(serde_json::from_value::<CurrentWriter>(invalid).is_err());
    }
    Ok(())
}
