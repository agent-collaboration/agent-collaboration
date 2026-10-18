use crate::{FrameId, MachineFrame, MalformedFrameDetail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
struct FrameCorrelation {
    kind: String,
    frame_id: Option<Value>,
}

/// Only request envelopes carry a recoverable correlation identity. Raw
/// payload values never cross this decode boundary or enter a refusal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MachineFrameDecodeFailure {
    InvalidRequest { frame_id: FrameId, field: String },
    Uncorrelated { detail: MalformedFrameDetail },
}

pub fn decode_machine_frame(bytes: &[u8]) -> Result<MachineFrame, MachineFrameDecodeFailure> {
    // Preserve the strict typed decoder (including duplicate-field rejection).
    // Only a failed frame needs a second, partial parse to recover its ID.
    if let Ok(frame) = serde_json::from_slice(bytes) {
        return Ok(frame);
    }
    let envelope: FrameCorrelation = serde_json::from_slice(bytes)
        .map_err(|_| uncorrelated(MalformedFrameDetail::Undecodable))?;
    if envelope.kind != "request" {
        return Err(uncorrelated(MalformedFrameDetail::Undecodable));
    }
    let frame_id = envelope
        .frame_id
        .and_then(|value| serde_json::from_value::<FrameId>(value).ok())
        .ok_or_else(|| uncorrelated(MalformedFrameDetail::RequestWithoutFrameId))?;
    Err(MachineFrameDecodeFailure::InvalidRequest {
        frame_id,
        field: "body".into(),
    })
}

fn uncorrelated(detail: MalformedFrameDetail) -> MachineFrameDecodeFailure {
    MachineFrameDecodeFailure::Uncorrelated { detail }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::test_fixtures::*;
    use crate::*;
    use serde_json::json;

    #[test]
    fn tail_digest_rejects_bad_shape_with_correlated_body_failure() {
        let correlation = frame_id();
        let packet = |digest: serde_json::Value| json!({"kind":"request","frame_id":correlation,"body":{"kind":"tailReport","value":{"project":project_id(),"epoch":1,"request_ids":[request_id()],"digest":digest}}});
        assert!(
            decode_machine_frame(
                &serde_json::to_vec(&packet(json!("ab".repeat(32)))).expect("valid packet")
            )
            .is_ok()
        );
        for digest in [
            json!(""),
            json!("AB".repeat(32)),
            json!("a".repeat(63)),
            json!("gg".repeat(32)),
            json!(42),
        ] {
            assert_eq!(
                decode_machine_frame(&serde_json::to_vec(&packet(digest)).expect("packet")),
                Err(MachineFrameDecodeFailure::InvalidRequest {
                    frame_id: correlation.clone(),
                    field: "body".into()
                })
            );
        }
    }

    #[test]
    fn valid_hello_and_forward_answer_keep_their_id_free_shapes() {
        for frame in [
            MachineFrame::Hello {
                installation: installation_id(),
                session_id: session_id(),
                schema_support: SchemaSupport { max_version: 1 },
            },
            MachineFrame::ForwardAnswer {
                forward_id: ForwardId::new(frame_id().as_str()).expect("ID"),
                reply: BoardPayload::new(json!({"accepted":true})),
            },
        ] {
            assert_eq!(
                decode_machine_frame(&serde_json::to_vec(&frame).expect("bytes")),
                Ok(frame)
            );
        }
    }

    #[test]
    fn valid_request_variants_still_decode_and_duplicate_fields_still_refuse() {
        for (body, _) in request_samples() {
            let frame = MachineFrame::Request {
                frame_id: frame_id(),
                body,
            };
            assert_eq!(
                decode_machine_frame(&serde_json::to_vec(&frame).expect("bytes")),
                Ok(frame)
            );
        }
        let request_identity = frame_id();
        let duplicate = format!(
            r#"{{"kind":"request","frame_id":"{}","body":{{"kind":"read","value":{{"scope":{{"kind":"notices"}},"after":null}}}},"body":{{"kind":"read","value":{{"scope":{{"kind":"notices"}},"after":null}}}}}}"#,
            request_identity.as_str()
        );
        assert_eq!(
            decode_machine_frame(duplicate.as_bytes()),
            Err(MachineFrameDecodeFailure::InvalidRequest {
                frame_id: request_identity,
                field: "body".into()
            })
        );
        let ambiguous = format!(
            r#"{{"kind":"request","frame_id":"{}","frame_id":"{}","body":null}}"#,
            frame_id().as_str(),
            frame_id().as_str()
        );
        assert_eq!(
            decode_machine_frame(ambiguous.as_bytes()),
            Err(uncorrelated(MalformedFrameDetail::Undecodable))
        );
    }

    #[test]
    fn only_requests_with_valid_ids_can_receive_correlated_refusals() {
        let frame_id = frame_id();
        for body in [
            json!(null),
            json!({"kind":"unknown"}),
            json!({"kind":"read","value":{"scope":{"kind":"projectCopy","value":{"project":project_id()}}}}),
        ] {
            let raw = json!({"kind":"request","frame_id":frame_id,"body":body});
            assert_eq!(
                decode_machine_frame(&serde_json::to_vec(&raw).expect("bytes")),
                Err(MachineFrameDecodeFailure::InvalidRequest {
                    frame_id: frame_id.clone(),
                    field: "body".into()
                })
            );
        }
        for raw in [
            json!({"kind":"request"}),
            json!({"kind":"request","frame_id":"wrong"}),
        ] {
            assert_eq!(
                decode_machine_frame(&serde_json::to_vec(&raw).expect("bytes")),
                Err(uncorrelated(MalformedFrameDetail::RequestWithoutFrameId))
            );
        }
        for raw in [b"{".as_slice(), br#"{"kind":"unknown"}"#.as_slice()] {
            assert_eq!(
                decode_machine_frame(raw),
                Err(uncorrelated(MalformedFrameDetail::Undecodable))
            );
        }
    }
}
