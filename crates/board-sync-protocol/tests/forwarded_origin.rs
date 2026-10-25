#![allow(clippy::unwrap_used)]
use board_sync_protocol::{BoardAnnotation, VerifiedForwardedPost};
use message_board::RecordTime;
use serde_json::json;

const PROJECT: &str = "019a1234-5678-7000-8000-000000000001";
const SENDER: &str = "00000000-0000-4000-8000-000000000001";

#[test]
fn verified_forward_keeps_a_v4_service_id_and_the_enrolled_label_without_a_peer_id() {
    let value = json!({"project":PROJECT,"origin":{"installation":SENDER,"machine":"enrolled sender"},"post":{"kind":"threadPost","body":{"actor":{"kind":"human","humanId":"owner"}}}});
    let decoded: VerifiedForwardedPost = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.origin.installation.as_str(), SENDER);
    assert_eq!(decoded.origin.machine.as_str(), "enrolled sender");
    assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
    for invalid in [
        json!({"project":PROJECT,"origin":PROJECT,"originLabel":"old shape","post":{"kind":"threadPost","body":{}}}),
        json!({"project":PROJECT,"origin":{"installation":SENDER},"post":{"kind":"threadPost","body":{}}}),
        json!({"project":PROJECT,"origin":{"installation":SENDER,"machine":"sender","peer":PROJECT},"post":{"kind":"threadPost","body":{}}}),
        json!({"project":PROJECT,"origin":{"installation":SENDER,"machine":"sender"},"originLabel":"second truth","post":{"kind":"threadPost","body":{}}}),
    ] {
        assert!(serde_json::from_value::<VerifiedForwardedPost>(invalid).is_err());
    }
}

#[test]
fn a_statusless_summary_is_an_annotation_with_explicit_none_not_an_absent_annotation() {
    let value =
        json!({"kind":"projectSummary","value":{"latestRecordAt":{"kind":"legacyUnknown"}}});
    let decoded: BoardAnnotation = serde_json::from_value(value.clone()).unwrap();
    assert!(matches!(
        decoded,
        BoardAnnotation::ProjectSummary {
            latest_record_at: RecordTime::LegacyUnknown
        }
    ));
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
}
