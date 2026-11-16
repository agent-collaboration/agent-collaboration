//! N9 tap recording: the duplex connection's frames exactly as they crossed, and the
//! Router-owned codecs those frames are held to.
use std::collections::HashMap;

use codex_router_test_support::node_taps::{MessageFamily, RecordIntent, RecordLayer, TapCase};
use serde_json::Value;

use super::{ACP_CLIENT, MARKER_METHOD, N4, RouteHarness, WireCrossing};

/// Holds the case's Router-owned records (the `_session/*` extensions, the `initialize` and session
/// result `_meta`) to the shared owning-check registry the live recording checker uses too.
pub(super) fn register_router_codecs(case: &mut TapCase) {
    case.use_owned_codecs(
        codex_router_test_support::node_taps::OwnedCodecs::from_owner_registry(
            acp_provider_adapters::owned_contract_checks::round_trips(),
            acp_provider_adapters::owned_contract_checks::verdicts(),
        )
        .expect("every listed contract is a Router contract"),
    );
}

/// Whether a recorded segment of the connection is the case's own exchange or fixture setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Segment {
    CaseExchange,
    Setup,
}

/// The kind string of what N9 sent to an ACP client.
fn frame_kind(frame: &Value, request_method: &str) -> String {
    if let Some(method) = frame.get("method").and_then(Value::as_str) {
        return match frame
            .pointer("/params/update/sessionUpdate")
            .and_then(Value::as_str)
        {
            Some(update) => format!("{method}:{update}"),
            None => method.to_owned(),
        };
    }
    match frame.pointer("/error/code") {
        Some(code) => format!("error:{code}:{request_method}"),
        None => format!("response:{request_method}"),
    }
}

/// Records the connection's frames since the last call, exactly as they crossed: first every
/// frame the client wrote to N9, then the session commands N9 issued after the first
/// `commands_skipped`, then every frame N9 wrote. Each request is so recorded before its reply;
/// N9 sends no request of its own in these cases, so no client reply follows one of N9's frames.
///
/// The marker exchange is always fixture setup; with [`Segment::Setup`] every record of the
/// segment is.
pub(super) fn record_connection(
    case: &mut TapCase,
    harness: &mut RouteHarness,
    commands_skipped: usize,
    segment: Segment,
) {
    let setup = segment == Segment::Setup;
    let marker_id = Value::from(super::MARKER_ID);
    let request_methods: HashMap<String, String> = harness
        .transcript
        .iter()
        .filter_map(|crossing| match crossing {
            WireCrossing::ToRoute(frame) => Some((
                frame["id"].to_string(),
                frame["method"].as_str().unwrap_or_default().to_owned(),
            )),
            WireCrossing::FromRoute(_) => None,
        })
        .collect();
    let unrecorded = &harness.transcript[harness.recorded..];
    for crossing in unrecorded {
        if let WireCrossing::ToRoute(frame) = crossing {
            let method = frame["method"].as_str().unwrap_or_default();
            case.consumed_raw(
                RecordLayer::WireFrame,
                MessageFamily::AcpServedForRouterSessions,
                ACP_CLIENT,
                method,
                frame.clone(),
                &[],
            );
            if method == MARKER_METHOD {
                case.mark_last(RecordIntent::SyntheticApparatus);
            }
            if setup || method == MARKER_METHOD {
                case.mark_last_setup();
            }
        }
    }
    for (command, detail) in harness
        .commands
        .commands()
        .into_iter()
        .skip(commands_skipped)
    {
        case.produced_with(
            RecordLayer::TypedOutcome,
            MessageFamily::SessionCommand,
            N4,
            &command,
            detail,
        );
        if setup {
            case.mark_last_setup();
        }
    }
    for crossing in unrecorded {
        if let WireCrossing::FromRoute(frame) = crossing {
            let request_method = request_methods
                .get(&frame["id"].to_string())
                .map_or("", String::as_str);
            let kind = frame_kind(frame, request_method);
            if frame.get("method").is_some() {
                case.produced_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpServedForRouterSessions,
                    ACP_CLIENT,
                    &kind,
                    frame.clone(),
                    &[],
                );
            } else {
                case.produced_reply_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpServedForRouterSessions,
                    ACP_CLIENT,
                    &kind,
                    frame.clone(),
                    &[],
                );
            }
            if setup || frame["id"] == marker_id {
                case.mark_last_setup();
            }
        }
    }
    harness.recorded = harness.transcript.len();
}
