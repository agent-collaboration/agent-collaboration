import type { LedgerSection } from "../schema-audit-types.ts";
// Part 2 of 2 by path order (session_account_affinity_cache.rs to websocket/*); part 1 is codex-router-proxy.ts.

export const section: LedgerSection = {
  root: "crates/codex-router-proxy",
  files: [
    {
      file: "crates/codex-router-proxy/src/session_account_affinity_cache.rs",
      disposition: { kind: "none" },
      note: "Process-local session-to-account pin cache keyed by provider and session-id header value (SessionAffinityKey L53), with Claude pin compare-and-set through the state repository (L332-L533) and DbWriteCommand enqueue L583; no message shapes read, built or sent. In-file #[cfg(test)] modules at L595-L597 and L599.",
    },
    {
      file: "crates/codex-router-proxy/src/telemetry.rs",
      disposition: { kind: "none" },
      note: "OpenTelemetry counters and gauges plus tracing events with scrubbed labels (L87-L314); no message shapes. In-file #[cfg(test)] mod tests at L316.",
    },
    {
      file: "crates/codex-router-proxy/src/test_log_capture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at lib.rs:18-19 (#[cfg(test)] mod test_log_capture)",
      },
    },
    {
      file: "crates/codex-router-proxy/src/tests/credential_generation_websocket.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:332-333, itself #[cfg(test)] mod tests at lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/tests/credit_affinity_transport.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:323-324, itself #[cfg(test)] mod tests at lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/tests/credit_compact_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at tests/credit_compact_transport.rs:3-4 (mod support), itself declared at proxy_tests.rs:326-327 under #[cfg(test)] mod tests at lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/tests/credit_compact_transport.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:326-327, itself #[cfg(test)] mod tests at lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/tests/credit_transport_proof.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:329-330, itself #[cfg(test)] mod tests at lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/upstream.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Upstream HTTP transport. chatgpt_backend_url_for_path L93-L116 is a hand path table rewriting the Router's /v1/responses, /v1/responses/compact, /v1/models, /v1/images/generations and /v1/images/edits to the ChatGPT backend codex/... paths. HyperHttpUpstreamTransport sends to the OpenAI or Anthropic upstream (L257-L263), which is not one of the six outside edges. The JSON bodies at L610 and L670 are inside #[cfg(test)] tests (L575, L677). OpenAI Responses model traffic relayed by the N2 byte relay, not the Codex app-server protocol, so outside Project 2's protocols: no inventory or send-site rows.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket.rs",
      disposition: { kind: "none" },
      note: "WebSocket routing composition: WebSocketFrame byte wrapper L138, handshake headers, close reasons, error enum; WebSocketProtocolRouter::ensure_first_frame_allowed L282-L293 calls the auth-carrier scan owned by response_metadata.rs:158. No serde shapes; frames carried as bytes.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/account_turn_admission.rs",
      disposition: { kind: "none" },
      note: "In-process floor-switch turn gate (FloorSwitchIntent L19, AccountTurnAdmission L25) driven by watch channels and cancellation tokens; no frames read or written.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/async_tunnel.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Async tunnel: sends CODEX_WEBSOCKET_RECONNECT_SIGNAL to the local Codex at L231 (floor reconnect while connecting upstream) and L273 (floor reconnect before the first frame is forwarded), and the pre-upstream Router signal at L323 chosen by the hand table pre_upstream_router_signal L328-L349. The first frame goes upstream at L289 and a Pong control frame at L372; neither is on one of the six outside edges. OpenAI Responses model traffic relayed by the N2 byte relay, not the Codex app-server protocol, so outside Project 2's protocols: no inventory or send-site rows.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/authenticated_routing.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Local auth, account selection and credential resolution for the first frame; rewraps the first frame's bytes as a POST /v1/responses HttpProxyRequest body (L121-L123, L344-L346) for the selector without parsing them here, and emits local audit events (L474-L514), which are not an outside edge. Reads the session-id handshake header into the selector request (L124, L347); model traffic, so no inventory row.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/blocking_tunnel.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket.rs:127-128 (#[cfg(test)] mod blocking_tunnel)",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/duplex_forwarding.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Duplex relay. Local-Codex sends: CODEX_WEBSOCKET_RECONNECT_SIGNAL at L328 (quota floor reconnect) and L338 (early floor reconnect); upstream frames, or the Router signal frame that maybe_replace_account_quota_exhaustion_with_reconnect_signal substituted, at L408 (terminal frame) and L420 (non-terminal frame). L296 sends local frames to the OpenAI upstream, not one of the six outside edges. Uses response_metadata parsers at L275, L370-L403. OpenAI Responses model traffic relayed by the N2 byte relay, not the Codex app-server protocol, so outside Project 2's protocols: no inventory or send-site rows.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/credit-included-peer-transition-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:5-6, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/credit-source-assessment-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:7-8, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/credit-turn-admission-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:9-10, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/credit-turn-depletion-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:11-12, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/credit-turn-source-transition-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:13-14, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/credit-turn-test-support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:15-16, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/credit-turn-unknown-transition-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:17-18, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/floor-switch-supervisor-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:19-20, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/floor-switch-terminal-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:21-22, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/floor-switch-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:23-24, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/handshake-cancellation-tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:25-26, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/handshake_record_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:29-30, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/forwarding_tests/provider_observer_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at websocket/forwarding_tests.rs:35-36, itself #[cfg(test)] mod forwarding_tests at websocket.rs:129-131",
      },
    },
    {
      file: "crates/codex-router-proxy/src/websocket/provider_signals.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Router-synthesized Responses WebSocket frames written as raw JSON strings: CODEX_WEBSOCKET_RECONNECT_SIGNAL L7, ROUTER_ALL_ACCOUNTS_EXHAUSTED_SIGNAL L8, ROUTER_NO_OPENAI_ACCOUNTS_SIGNAL L9, ROUTER_QUOTA_STATE_UNAVAILABLE_SIGNAL L10, short_quota_wait_signal L14-L18 (format! into a response.failed frame); maybe_replace_account_quota_exhaustion_with_reconnect_signal L35-L182 maps PostExhaustionRouteBandOutcome and observer failures to one of them. Nothing is sent here: the frames are returned as Message values and sent at duplex_forwarding.rs:408/420 and async_tunnel.rs:231/273/323. OpenAI Responses model traffic relayed by the N2 byte relay, not the Codex app-server protocol, so outside Project 2's protocols: no inventory or send-site rows.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/response_metadata.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Reads WebSocket text frames for routing facts: response.id parse L75-L89 (serde_json::from_str L79) into PreviousResponseId; response.create recognition L99-L108 on local frames; bounded top-level type scan L110-L120 (response.completed, response.failed, response.incomplete) via bounded_top_level_json_string_field L179; terminal-error check L122-L156 (top-level type error, status or status_code non-success, error.code websocket_connection_limit_reached or previous_response_not_found); forbidden auth-carrier top-level key scan L158-L166 with canonicalization L291-L305; error classification delegated to provider_error at L392. OpenAI Responses model traffic relayed by the N2 byte relay, not the Codex app-server protocol, so outside Project 2's protocols: no inventory or send-site rows.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/session_registry.rs",
      disposition: { kind: "none" },
      note: "In-process WebSocket session registry, cancellation and counters (WebSocketRevocationRegistry L17, WebSocketRegistrySnapshot L93 has no serde derive); no frames read or written.",
    },
    {
      file: "crates/codex-router-proxy/src/websocket/transport_cleanup.rs",
      disposition: { kind: "none" },
      note: "WebSocket config and best-effort close helpers (L6-L56); close frames only, no message shapes.",
    },
  ],
  sendSites: [],
};
