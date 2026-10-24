//! The Rust metadata oracle against known-good and wrong evidence: metadata shown as
//! prose, prose missing from history or the terminal, raw keys split across a wrap.
use crate::metadata_oracle::check_metadata_observation;
use crate::oracle_test_support::{
    SKILL, agent, catalogue, expect_error, item, observation, thread, user,
};
use crate::verification_models::HistoryThread;
use serde_json::Value;

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

#[test]
fn metadata_outside_genuine_prose_passes() {
    let observed = observation(metadata_events(), "deadlineReached");
    let result =
        check_metadata_observation(&observed, &metadata_views(), CAPTURE, CAPTURE, ORDINARY)
            .unwrap();
    assert_eq!(result.command_catalogues, 1);
    assert_eq!(result.usage_items, 1);
    assert_eq!(result.assistant_item_ids, ["reply"]);
    // The composer may follow the reply on the same captured row.
    let same_row = "• STEER_BASE_OK › Ask Codex to do anything";
    check_metadata_observation(&observed, &metadata_views(), same_row, CAPTURE, ORDINARY).unwrap();
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
            CAPTURE,
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
            CAPTURE,
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
            "said STEER_BASE_OK_LATER\n",
            CAPTURE,
            ORDINARY,
        ),
        "absent from original installed-client PTY",
    );
    // Raw keys, even split across a terminal wrap, are raw provider metadata.
    let raw = format!("{CAPTURE}\"input\nTokens\": 12");
    expect_error(
        check_metadata_observation(&observed, &metadata_views(), CAPTURE, &raw, ORDINARY),
        "raw provider metadata",
    );
    let no_catalogue = observation(metadata_events()[1..].to_vec(), "deadlineReached");
    expect_error(
        check_metadata_observation(&no_catalogue, &metadata_views(), CAPTURE, CAPTURE, ORDINARY),
        "not retained",
    );
    let incomplete = observation(metadata_events(), "resultLimitReached");
    expect_error(
        check_metadata_observation(&incomplete, &metadata_views(), CAPTURE, CAPTURE, ORDINARY),
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
        check_metadata_observation(&observed, &metadata_views(), &prose, CAPTURE, ORDINARY),
        "terminal prose",
    );
}
