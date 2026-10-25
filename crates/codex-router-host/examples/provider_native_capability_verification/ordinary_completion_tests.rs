//! J2 selected steer: the ordinary turn's native completion, judged from public
//! provider events because Codex 0.163 may stop drawing it after a failed steer.
use crate::conversation_oracle::{any_turn_ended, check_ordinary_completion};
use crate::oracle_test_support::{expect_error, item, observation, turn_started};
use serde_json::{Value, json};

fn turn_ended(turn_id: &str, kind: &str, stop_reason: Option<&str>) -> Value {
    json!({"kind": "turnEnded", "turnId": turn_id,
        "outcome": {"kind": kind, "stop_reason": stop_reason, "local_cause": null}})
}

#[test]
fn ordinary_turn_completion_is_judged_from_public_events() {
    // The observed shape: turnStarted, agentMessage STEER_BASE_OK, turnEnded endTurn.
    let completed = observation(
        vec![
            turn_started("ordinary"),
            item(
                "thought",
                "agentThought",
                "The response will be STEER_BASE_OK.",
            ),
            item("reply", "agentMessage", "STEER_BASE_OK"),
            turn_ended("ordinary", "ended", Some("endTurn")),
        ],
        "deadlineReached",
    );
    assert!(any_turn_ended(&completed));
    let ordinary = check_ordinary_completion(&completed, "STEER_BASE_OK").unwrap();
    assert_eq!(
        (ordinary.turn_id.as_str(), ordinary.reply_item_id.as_str()),
        ("ordinary", "reply")
    );
    let wrong = [
        (
            vec![
                turn_started("ordinary"),
                turn_ended("ordinary", "ended", Some("endTurn")),
            ],
            "reply",
        ),
        (
            vec![
                turn_started("ordinary"),
                item("reply", "agentMessage", "STEER_BASE"),
                turn_ended("ordinary", "ended", Some("endTurn")),
            ],
            "reply",
        ),
        (
            vec![
                turn_started("ordinary"),
                item("reply", "agentMessage", "STEER_BASE_OK"),
                item("again", "agentMessage", "STEER_BASE_OK"),
                turn_ended("ordinary", "ended", Some("endTurn")),
            ],
            "reply",
        ),
        (
            vec![
                turn_started("ordinary"),
                turn_started("second"),
                item("reply", "agentMessage", "STEER_BASE_OK"),
                turn_ended("ordinary", "ended", Some("endTurn")),
            ],
            "exactly one ordinary turn",
        ),
        (
            vec![
                turn_started("ordinary"),
                item("reply", "agentMessage", "STEER_BASE_OK"),
            ],
            "no single end",
        ),
        (
            vec![
                turn_started("ordinary"),
                item("reply", "agentMessage", "STEER_BASE_OK"),
                turn_ended("ordinary", "ended", Some("cancelled")),
            ],
            "did not end normally",
        ),
        (
            vec![
                turn_started("ordinary"),
                item("reply", "agentMessage", "STEER_BASE_OK"),
                turn_ended("ordinary", "refused", None),
            ],
            "did not end normally",
        ),
    ];
    for (events, fragment) in wrong {
        expect_error(
            check_ordinary_completion(&observation(events, "deadlineReached"), "STEER_BASE_OK"),
            fragment,
        );
    }
    expect_error(
        check_ordinary_completion(&observation(vec![], "resultLimitReached"), "STEER_BASE_OK"),
        "incomplete",
    );
    assert!(!any_turn_ended(&observation(
        vec![turn_started("t")],
        "deadlineReached"
    )));
}
