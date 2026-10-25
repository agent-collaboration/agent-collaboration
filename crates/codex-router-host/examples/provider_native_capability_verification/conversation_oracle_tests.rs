//! J2-J4 oracles against known-good and wrong evidence.
//!
//! Each wrong case is a build the Verification Design says must fail: a silently
//! dropped selection, an echoed canary, a changed history command, display-driven
//! operations, metadata shown as prose. These establish apparatus sensitivity only,
//! never product proof.
use crate::conversation_oracle::{
    QueueObservation, check_command_catalogues, check_display_operations, check_native_invocation,
    check_recorded_selection, check_rejected_input, recorded_user_turns, require_fresh_session,
    selected_steer_raced,
};
use crate::oracle_test_support::{
    ARGUMENTS, CANARY, NativeSetup, SECOND_CANARY, SECOND_SKILL, SKILL, catalogue, expect_error,
    item, observation, operation, turn_started,
};
use serde_json::{Value, json};

const PROMPT: &str = "conversationPrompt";

fn native_reply() -> String {
    format!("{CANARY}::{ARGUMENTS}")
}

#[test]
fn latest_list_replaces_earlier_entries() {
    let setup = NativeSetup::new();
    let events = vec![
        catalogue("first", &["compact", "old", SKILL]),
        catalogue("second", &["compact", SKILL]),
    ];
    let result =
        check_command_catalogues(&observation(events, "deadlineReached"), &setup.fixture).unwrap();
    assert!(result.replacement_observed);
    assert_eq!(result.removed_names, ["old"]);
    assert_eq!(result.current_names, ["compact", SKILL]);
}

#[test]
fn identical_lists_leave_replacement_unobserved() {
    let setup = NativeSetup::new();
    let events = vec![catalogue("first", &[SKILL]), catalogue("second", &[SKILL])];
    let result =
        check_command_catalogues(&observation(events, "deadlineReached"), &setup.fixture).unwrap();
    assert!(!result.replacement_observed);
}

#[test]
fn current_list_without_the_native_command_is_rejected() {
    let setup = NativeSetup::new();
    let events = vec![
        catalogue("first", &[SKILL]),
        catalogue("second", &["compact"]),
    ];
    assert!(
        check_command_catalogues(&observation(events, "deadlineReached"), &setup.fixture).is_err()
    );
    let incomplete = observation(vec![catalogue("first", &[SKILL])], "resultLimitReached");
    expect_error(
        check_command_catalogues(&incomplete, &setup.fixture),
        "incomplete",
    );
}

#[test]
fn argument_dependent_native_effect_passes() {
    let setup = NativeSetup::new();
    let events = observation(
        vec![
            turn_started("turn-1"),
            item("reply", "agentMessage", &native_reply()),
        ],
        "deadlineReached",
    );
    let result = check_native_invocation(
        &events,
        &setup.fixture,
        ARGUMENTS,
        &[format!("${SKILL} {ARGUMENTS}")],
        &[],
        &[operation("prompt-1", PROMPT, 1, Some(2))],
    )
    .unwrap();
    assert_eq!(result.reply_item_id, "reply");
}

#[test]
fn dropped_corrupted_or_echoed_selection_is_rejected() {
    let setup = NativeSetup::new();
    let cases = [
        ("Sure, done.".to_owned(), "input".to_owned()),
        (
            format!("{CANARY}::literal-value café\nsecond-line"),
            "input".to_owned(),
        ),
        (native_reply(), format!("{CANARY} {ARGUMENTS}")),
    ];
    for (reply, supplied) in cases {
        let events = observation(
            vec![
                turn_started("turn-1"),
                item("reply", "agentMessage", &reply),
            ],
            "deadlineReached",
        );
        let result = check_native_invocation(
            &events,
            &setup.fixture,
            ARGUMENTS,
            &[supplied],
            &[],
            &[operation("p", PROMPT, 1, Some(2))],
        );
        assert!(result.is_err(), "{reply:?}");
    }
}

#[test]
fn positive_body_claims_need_a_fresh_session() {
    let setup = NativeSetup::new();
    let fresh = observation(
        vec![
            catalogue("catalogue", &[SKILL]),
            item("prior", "agentMessage", "hello"),
        ],
        "deadlineReached",
    );
    require_fresh_session(&fresh, &setup.fixture).unwrap();
    // Earlier history already holding either canary (a reply, or the body echoed into
    // context) means a later effect may come from that context, not the native body.
    let contaminated = [
        item("prior-reply", "agentMessage", &native_reply()),
        item(
            "prior-user",
            "userMessage",
            &format!("remember {SECOND_CANARY}"),
        ),
    ];
    for prior in contaminated {
        let observed = observation(
            vec![catalogue("catalogue", &[SKILL]), prior],
            "deadlineReached",
        );
        expect_error(
            require_fresh_session(&observed, &setup.fixture),
            "not a fresh Session",
        );
    }
    expect_error(
        require_fresh_session(&observation(vec![], "resultLimitReached"), &setup.fixture),
        "incomplete",
    );
}

#[test]
fn invocation_needs_exactly_one_router_prompt() {
    let setup = NativeSetup::new();
    let events = observation(
        vec![
            turn_started("turn-1"),
            item("reply", "agentMessage", &native_reply()),
        ],
        "deadlineReached",
    );
    expect_error(
        check_native_invocation(&events, &setup.fixture, ARGUMENTS, &[], &[], &[]),
        "exactly one Router prompt",
    );
    let two = [
        operation("a", PROMPT, 1, Some(2)),
        operation("b", PROMPT, 3, Some(4)),
    ];
    expect_error(
        check_native_invocation(&events, &setup.fixture, ARGUMENTS, &[], &[], &two),
        "exactly one Router prompt",
    );
}

#[test]
fn explicit_rejection_with_no_native_effect_passes() {
    let setup = NativeSetup::new();
    let result = check_rejected_input(
        &observation(vec![], "deadlineReached"),
        &setup.fixture,
        &[],
        &[],
        &unchanged_queue(),
        true,
        0,
    );
    assert_eq!(result.unwrap(), Vec::<String>::new());
}

#[test]
fn silent_drop_is_not_an_explicit_rejection() {
    let setup = NativeSetup::new();
    expect_error(
        check_rejected_input(
            &observation(vec![], "deadlineReached"),
            &setup.fixture,
            &[],
            &[],
            &unchanged_queue(),
            false,
            0,
        ),
        "dropped silently",
    );
}

#[test]
fn rejected_selection_that_reached_the_provider_fails() {
    let setup = NativeSetup::new();
    let reached = [
        (
            observation(vec![], "deadlineReached"),
            vec![operation("prompt-1", PROMPT, 1, Some(2))],
        ),
        (
            observation(vec![turn_started("turn-1")], "deadlineReached"),
            vec![],
        ),
        (
            observation(
                vec![item(
                    "reply",
                    "agentMessage",
                    &format!("{SECOND_CANARY}::x"),
                )],
                "deadlineReached",
            ),
            vec![],
        ),
        (observation(vec![], "resultLimitReached"), vec![]),
    ];
    for (events, operations) in reached {
        assert!(
            check_rejected_input(
                &events,
                &setup.fixture,
                &[],
                &operations,
                &unchanged_queue(),
                true,
                0,
            )
            .is_err()
        );
    }
}

/// A queue that already held one unrelated input and still holds exactly it.
fn unchanged_queue() -> QueueObservation {
    QueueObservation {
        before: vec!["input-earlier".to_owned()],
        after: vec!["input-earlier".to_owned()],
    }
}

#[test]
fn rejected_selection_left_waiting_in_the_queue_fails() {
    // Prompt and turn counts stay unchanged while a queued selection waits, so only
    // the delivery queue itself can show it.
    let setup = NativeSetup::new();
    let quiet = observation(vec![], "deadlineReached");
    let changed = [
        QueueObservation {
            before: vec!["input-earlier".to_owned()],
            after: vec!["input-earlier".to_owned(), "input-selected".to_owned()],
        },
        QueueObservation {
            before: vec![],
            after: vec!["input-selected".to_owned()],
        },
        // Same count, different queued input: one left and the selection took its place.
        QueueObservation {
            before: vec!["input-earlier".to_owned()],
            after: vec!["input-selected".to_owned()],
        },
    ];
    for queue in changed {
        expect_error(
            check_rejected_input(&quiet, &setup.fixture, &[], &[], &queue, true, 0),
            "changed the Router delivery queue",
        );
    }
    check_rejected_input(
        &quiet,
        &setup.fixture,
        &[],
        &[],
        &unchanged_queue(),
        true,
        0,
    )
    .unwrap();
}

#[test]
fn selected_steer_allows_only_its_ordinary_turn() {
    let setup = NativeSetup::new();
    let events = observation(
        vec![
            turn_started("turn-1"),
            item("reply", "agentMessage", "STEER_BASE_OK"),
        ],
        "deadlineReached",
    );
    let result = check_rejected_input(
        &events,
        &setup.fixture,
        &[],
        &[operation("prompt-1", PROMPT, 1, Some(2))],
        &unchanged_queue(),
        true,
        1,
    );
    assert_eq!(result.unwrap(), ["prompt-1"]);
}

#[test]
fn steer_admitted_after_the_ordinary_turn_ended_is_a_race() {
    let ordinary = operation("ordinary", PROMPT, 10, Some(20));
    // The selected input reached Router while the ordinary turn was still running.
    assert!(!selected_steer_raced(
        &[],
        &[ordinary.clone(), operation("steer", PROMPT, 15, Some(25))]
    ));
    // It arrived only after that turn ended: a fresh turn, so the case never ran.
    assert!(selected_steer_raced(
        &[],
        &[ordinary.clone(), operation("late", PROMPT, 21, Some(30))]
    ));
    assert!(!selected_steer_raced(
        &[],
        &[
            operation("running", PROMPT, 10, None),
            operation("x", PROMPT, 21, None)
        ]
    ));
    assert!(!selected_steer_raced(&[], &[ordinary]));
}

fn recorder(items: Vec<Value>) -> Vec<Value> {
    vec![
        json!({"dir": "meta", "kind": "session_start"}),
        json!({"dir": "to_tui", "kind": "app_event", "variant": "CommitTick", "items": [{"type": "text"}]}),
        json!({"dir": "from_tui", "kind": "op", "payload": {"UserTurn": {"items": items, "cwd": "/w"}}}),
    ]
}

fn selected_text() -> String {
    format!("${SKILL} {ARGUMENTS}")
}

#[test]
fn client_text_and_skill_with_real_origin_pass() {
    let setup = NativeSetup::new();
    let records = recorder(vec![
        json!({"type": "text", "text": selected_text(), "text_elements": [{"byte_range": {"start": 0, "end": 24}}]}),
        json!({"type": "skill", "name": SKILL, "path": setup.skill_file}),
    ]);
    let result =
        check_recorded_selection(&records, &setup.fixture, &selected_text(), &[SKILL], 1).unwrap();
    assert_eq!(result.item_types, ["text", "skill"]);
    assert_eq!(result.text_element_count, 1);
    // Only the client's own outbound op records count, never other recorder lines.
    assert_eq!(recorded_user_turns(&records).len(), 1);
}

#[test]
fn two_selected_skills_each_carry_their_own_origin() {
    let setup = NativeSetup::new();
    let text = format!("${SKILL} ${SECOND_SKILL} {ARGUMENTS}");
    let records = recorder(vec![
        json!({"type": "text", "text": text}),
        json!({"type": "skill", "name": SKILL, "path": setup.skill_file}),
        json!({"type": "skill", "name": SECOND_SKILL, "path": setup.second_file.parent().unwrap()}),
    ]);
    check_recorded_selection(&records, &setup.fixture, &text, &[SKILL, SECOND_SKILL], 1).unwrap();
    let swapped = recorder(vec![
        json!({"type": "text", "text": text}),
        json!({"type": "skill", "name": SKILL, "path": setup.second_file}),
        json!({"type": "skill", "name": SECOND_SKILL, "path": setup.skill_file}),
    ]);
    assert!(
        check_recorded_selection(&swapped, &setup.fixture, &text, &[SKILL, SECOND_SKILL], 1)
            .is_err()
    );
}

#[test]
fn case_not_reached_by_the_client_is_rejected() {
    let setup = NativeSetup::new();
    let text = json!({"type": "text", "text": selected_text()});
    let skill = json!({"type": "skill", "name": SKILL, "path": setup.skill_file});
    let wrong = [
        vec![text.clone()],
        vec![
            text.clone(),
            json!({"type": "skill", "name": SKILL, "path": setup.second_file}),
        ],
        vec![json!({"type": "text", "text": "different"}), skill],
        vec![
            text,
            json!({"type": "mention", "name": "Alpha", "path": "plugin://alpha@m"}),
        ],
    ];
    for items in wrong {
        assert!(
            check_recorded_selection(
                &recorder(items.clone()),
                &setup.fixture,
                &selected_text(),
                &[SKILL],
                1
            )
            .is_err(),
            "{items:?}"
        );
    }
}

#[test]
fn turn_count_must_match_the_case() {
    let setup = NativeSetup::new();
    let records = recorder(vec![
        json!({"type": "text", "text": selected_text()}),
        json!({"type": "skill", "name": SKILL, "path": setup.skill_file}),
    ]);
    expect_error(
        check_recorded_selection(&records, &setup.fixture, &selected_text(), &[SKILL], 2),
        "expected 2 recorded user turns",
    );
}

#[test]
fn pure_display_adds_nothing() {
    let before = [operation("prompt-1", PROMPT, 1, Some(2))];
    let result = check_display_operations(
        &before,
        &before,
        &observation(vec![], "deadlineReached"),
        false,
    );
    assert_eq!(result.unwrap(), Vec::<String>::new());
}

#[test]
fn display_driven_operations_or_turns_are_rejected() {
    let before = vec![operation("prompt-1", PROMPT, 1, Some(2))];
    let mut loaded = before.clone();
    loaded.push(operation("load", "conversationLoad", 3, Some(4)));
    let mut replayed = before.clone();
    replayed.push(operation("replayed", PROMPT, 3, Some(4)));
    let quiet = observation(vec![], "deadlineReached");
    expect_error(
        check_display_operations(&before, &loaded, &quiet, false),
        "conversationLoad",
    );
    expect_error(
        check_display_operations(&before, &replayed, &quiet, true),
        "conversationPrompt",
    );
    expect_error(
        check_display_operations(
            &before,
            &before,
            &observation(vec![turn_started("t")], "deadlineReached"),
            false,
        ),
        "provider turn",
    );
    expect_error(
        check_display_operations(
            &before,
            &before,
            &observation(vec![], "resultLimitReached"),
            false,
        ),
        "incomplete",
    );
}

#[test]
fn explicit_unloaded_resume_may_load() {
    let before = vec![operation("prompt-1", PROMPT, 1, Some(2))];
    let mut loaded = before.clone();
    loaded.push(operation("load", "conversationLoad", 3, Some(4)));
    let result = check_display_operations(
        &before,
        &loaded,
        &observation(vec![], "deadlineReached"),
        true,
    );
    assert_eq!(result.unwrap(), ["conversationLoad"]);
}
