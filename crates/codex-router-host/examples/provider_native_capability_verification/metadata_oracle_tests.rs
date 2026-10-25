//! The Rust metadata oracle against known-good and wrong evidence: metadata shown as
//! prose, prose missing from history or the terminal, raw keys split across a wrap, and
//! live-versus-reopened provenance of the original live capture, and where its offset
//! came from.
use crate::metadata_oracle::{LiveCapture, LiveOffset, check_metadata_observation};
use crate::oracle_test_support::{
    SKILL, agent, catalogue, expect_error, item, observation, thread, user,
};
use crate::verification_models::HistoryThread;
use serde_json::{Value, json};

const ORDINARY: &str = "STEER_BASE_OK";

fn metadata_events() -> Vec<Value> {
    vec![
        catalogue("catalogue", &[SKILL]),
        item(
            "usage",
            "usage",
            r#"{"inputTokens": 12, "outputTokens": 3}"#,
        ),
        item("reply", "agentMessage", ORDINARY),
    ]
}

fn metadata_views() -> Vec<HistoryThread> {
    let view = thread(
        "thread",
        vec![user("user", "ordinary prompt"), agent("reply", ORDINARY)],
    );
    vec![view.clone(), view]
}

const CAPTURE: &str = "› ordinary prompt\n\n• STEER_BASE_OK\n\n› Ask Codex to do anything\n";

/// A whole capture whose input starts at byte 0 (nothing redrawn before it).
fn live(capture: &str) -> LiveCapture<'_> {
    LiveCapture {
        raw: capture.as_bytes(),
        offset: LiveOffset::operator_supplied(0),
    }
}

#[test]
fn metadata_outside_genuine_prose_passes() {
    let observed = observation(metadata_events(), "deadlineReached");
    let result = check_metadata_observation(
        &observed,
        &metadata_views(),
        &live(CAPTURE),
        CAPTURE,
        ORDINARY,
    )
    .unwrap();
    assert_eq!(result.command_catalogues, 1);
    assert_eq!(result.usage_items, 1);
    assert_eq!(result.assistant_item_ids, ["reply"]);
    // The composer may follow the reply on the same captured row.
    let same_row = "• STEER_BASE_OK › Ask Codex to do anything";
    check_metadata_observation(
        &observed,
        &metadata_views(),
        &live(same_row),
        CAPTURE,
        ORDINARY,
    )
    .unwrap();
}

#[test]
fn metadata_shown_as_prose_or_missing_prose_is_rejected() {
    let observed = observation(metadata_events(), "deadlineReached");
    let leaked = thread(
        "thread",
        vec![
            user("user", "ordinary prompt"),
            agent("reply", ORDINARY),
            agent("usage", "{}"),
        ],
    );
    expect_error(
        check_metadata_observation(
            &observed,
            &[leaked.clone(), leaked],
            &live(CAPTURE),
            CAPTURE,
            ORDINARY,
        ),
        "leaked into a Codex transcript",
    );
    let missing = thread("thread", vec![user("user", "ordinary prompt")]);
    expect_error(
        check_metadata_observation(
            &observed,
            &[missing.clone(), missing],
            &live(CAPTURE),
            CAPTURE,
            ORDINARY,
        ),
        "missing from Codex history",
    );
    // A reply text that only appears inside other text is not an assistant row.
    expect_error(
        check_metadata_observation(
            &observed,
            &metadata_views(),
            &live("said STEER_BASE_OK_LATER\n"),
            CAPTURE,
            ORDINARY,
        ),
        "absent from original live PTY",
    );
    expect_error(
        check_metadata_observation(
            &observed,
            &metadata_views(),
            &live(CAPTURE),
            "said STEER_BASE_OK_LATER\n",
            ORDINARY,
        ),
        "absent from reopened",
    );
    // Raw keys, even split across a terminal wrap, are raw provider metadata.
    let raw = format!("{CAPTURE}\"input\nTokens\": 12");
    expect_error(
        check_metadata_observation(&observed, &metadata_views(), &live(CAPTURE), &raw, ORDINARY),
        "raw provider metadata",
    );
    let no_catalogue = observation(metadata_events()[1..].to_vec(), "deadlineReached");
    expect_error(
        check_metadata_observation(
            &no_catalogue,
            &metadata_views(),
            &live(CAPTURE),
            CAPTURE,
            ORDINARY,
        ),
        "not retained",
    );
    let incomplete = observation(metadata_events(), "resultLimitReached");
    expect_error(
        check_metadata_observation(
            &incomplete,
            &metadata_views(),
            &live(CAPTURE),
            CAPTURE,
            ORDINARY,
        ),
        "incomplete",
    );
}

#[test]
fn plain_text_metadata_rendered_as_an_assistant_row_is_rejected() {
    let mut events = metadata_events();
    events.push(item("mode", "modeChange", "plan mode"));
    let observed = observation(events, "deadlineReached");
    let prose = format!("{CAPTURE}• plan mode\n");
    expect_error(
        check_metadata_observation(
            &observed,
            &metadata_views(),
            &live(&prose),
            CAPTURE,
            ORDINARY,
        ),
        "terminal prose",
    );
}

/// What a resumed client draws before any input: the earlier turn's reply, redrawn.
const REPLAYED_PREFIX: &str = "• STEER_BASE_OK  › Ask Codex to do anything\n";

#[test]
fn a_replayed_reply_before_the_input_is_never_the_live_reply() {
    let observed = observation(metadata_events(), "deadlineReached");
    // Replay-prefix only: the new input is drawn, the new reply never is.
    let replay_only = format!("{REPLAYED_PREFIX}› ordinary prompt\n");
    let offset = REPLAYED_PREFIX.len();
    expect_error(
        check_metadata_observation(
            &observed,
            &metadata_views(),
            &LiveCapture {
                raw: replay_only.as_bytes(),
                offset: LiveOffset::operator_supplied(offset),
            },
            CAPTURE,
            ORDINARY,
        ),
        "absent from original live PTY after the input",
    );
    // Judged whole (offset 0), the same bytes would wrongly pass: the offset matters.
    check_metadata_observation(
        &observed,
        &metadata_views(),
        &LiveCapture {
            raw: replay_only.as_bytes(),
            offset: LiveOffset::operator_supplied(0),
        },
        CAPTURE,
        ORDINARY,
    )
    .unwrap();
    // A new reply drawn after the input passes.
    let live_reply = format!("{REPLAYED_PREFIX}› ordinary prompt\n\n• STEER_BASE_OK\n");
    check_metadata_observation(
        &observed,
        &metadata_views(),
        &LiveCapture {
            raw: live_reply.as_bytes(),
            offset: LiveOffset::operator_supplied(offset),
        },
        CAPTURE,
        ORDINARY,
    )
    .unwrap();
    // An offset past the capture is rejected, not clamped.
    expect_error(
        check_metadata_observation(
            &observed,
            &metadata_views(),
            &LiveCapture {
                raw: live_reply.as_bytes(),
                offset: LiveOffset::operator_supplied(live_reply.len() + 1),
            },
            CAPTURE,
            ORDINARY,
        ),
        "outside the",
    );
}

#[test]
fn metadata_leaks_before_the_input_are_still_found() {
    let observed = observation(metadata_events(), "deadlineReached");
    // A raw usage key in the redrawn prefix, before the offset, still fails.
    let prefix = "{\"inputTokens\": 12}\n";
    let capture = format!("{prefix}› ordinary prompt\n\n• STEER_BASE_OK\n");
    expect_error(
        check_metadata_observation(
            &observed,
            &metadata_views(),
            &LiveCapture {
                raw: capture.as_bytes(),
                offset: LiveOffset::operator_supplied(prefix.len()),
            },
            CAPTURE,
            ORDINARY,
        ),
        "raw provider metadata",
    );
}

#[test]
fn the_offset_is_reported_with_its_provenance_and_judged_alike() {
    let observed = observation(metadata_events(), "deadlineReached");
    let replay_only = format!("{REPLAYED_PREFIX}› ordinary prompt\n");
    let live_reply = format!("{REPLAYED_PREFIX}› ordinary prompt\n\n• STEER_BASE_OK\n");
    let after_prefix = REPLAYED_PREFIX.len();
    for offset in [
        LiveOffset::operator_supplied(after_prefix),
        LiveOffset::recorded_by_driver(after_prefix),
    ] {
        // Provenance labels the evidence; it never relaxes the post-offset replay check.
        expect_error(
            check_metadata_observation(
                &observed,
                &metadata_views(),
                &LiveCapture {
                    raw: replay_only.as_bytes(),
                    offset,
                },
                CAPTURE,
                ORDINARY,
            ),
            "absent from original live PTY after the input",
        );
        let result = check_metadata_observation(
            &observed,
            &metadata_views(),
            &LiveCapture {
                raw: live_reply.as_bytes(),
                offset,
            },
            CAPTURE,
            ORDINARY,
        )
        .unwrap();
        assert_eq!(result.live_offset, offset);
    }
    // The same byte count from the operator and from the driver is different evidence.
    assert_ne!(
        LiveOffset::operator_supplied(after_prefix),
        LiveOffset::recorded_by_driver(after_prefix)
    );
    assert_eq!(
        serde_json::to_value(LiveOffset::operator_supplied(7)).unwrap(),
        json!({"bytes": 7, "provenance": "operatorSupplied"})
    );
    assert_eq!(
        serde_json::to_value(LiveOffset::recorded_by_driver(7)).unwrap(),
        json!({"bytes": 7, "provenance": "recordedByDriver"})
    );
}
