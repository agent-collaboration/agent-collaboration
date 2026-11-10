use super::{
    ExitStatus, MockHttpSseTranscript, MockWebSocketTranscript, RouterAuditObservation,
    RouterProcessObservation, SmokeQuotaStatus, SmokeTempRoot, upstream_account_token,
};
use std::os::unix::process::ExitStatusExt;

pub(super) fn success_status() -> ExitStatus {
    ExitStatus::from_raw(0)
}

pub(super) fn valid_transcript(
    local_token_in_http_body: bool,
    local_token_in_first_frame: bool,
) -> MockWebSocketTranscript {
    let local_token = "local-token-canary";
    let first_frame = if local_token_in_first_frame {
        format!(
            r#"{{"model":"gpt-5.5","input":[{{"role":"user","content":[{{"type":"input_text","text":"hello"}}]}}],"stream":true,"token":"{local_token}"}}"#
        )
    } else {
        r#"{"model":"gpt-5.5","input":[{"role":"user","content":[{"type":"input_text","text":"hello"}]}],"stream":true}"#.to_owned()
    };
    MockWebSocketTranscript {
        headers: vec![(
            "authorization".to_owned(),
            format!("Bearer {}", upstream_account_token()),
        )],
        first_frame: first_frame.clone(),
        request_frames: vec![first_frame],
        websocket_request_frame_count: 1,
        http_probe_count: 0,
        http_sse: Some(MockHttpSseTranscript {
            request_line: "POST /v1/responses HTTP/1.1".to_owned(),
            headers: vec![(
                "authorization".to_owned(),
                format!("Bearer {}", upstream_account_token()),
            )],
            body: if local_token_in_http_body {
                format!(r#"{{"stream":true,"token":"{local_token}"}}"#)
            } else {
                r#"{"stream":true}"#.to_owned()
            },
        }),
    }
}

pub(super) fn valid_quota_status() -> SmokeQuotaStatus {
    SmokeQuotaStatus {
        table: "matches 5h weekly resets available routing next use\n".to_owned(),
        plain: "account\tstatus\t5h\tweekly\tresets available\trouting\tnext use\nmatches\tenabled\t██████████ 91% resets in 4h\t██████░░░░ 54% resets in 6d\t-\t✓ preferred 5h 91%\tnext\nresponses route\tnext: matches\twhy: ✓ preferred 5h 91%\n".to_owned(),
        json: r#"{"preferred_next_account_hash":"hash_matches","accounts":[{"account_hash":"hash_matches","safe_account_label":"matches","preferred_next":true}]}"#.to_owned(),
    }
}

pub(super) fn valid_router_audit_observation() -> RouterAuditObservation {
    RouterAuditObservation {
        http_sse_local_auth_validated: true,
        websocket_local_auth_validated: true,
    }
}

pub(super) fn valid_router_process_observation(
    test_root: &SmokeTempRoot,
) -> RouterProcessObservation {
    RouterProcessObservation {
        binary_path: test_root.path().join("target/debug/codex-router"),
        pid: 42,
        argv: vec!["serve".to_owned(), "--port".to_owned(), "8787".to_owned()],
        listener: "127.0.0.1:8787".to_owned(),
        readiness_line: "listening: 127.0.0.1:8787".to_owned(),
        cleanup_result: "terminated:signal: 9 (SIGKILL)".to_owned(),
    }
}
