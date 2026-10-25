//! J4 canonical history against the original retained user identity/content set.
//! Apparatus sensitivity only, never native product proof. `ARGUMENTS` carries a
//! literal `$`, non-ASCII text and an internal newline, so byte changes are visible.
use crate::history_oracle::check_selected_history;
use crate::oracle_test_support::{
    ARGUMENTS, CANARY, NativeSetup, SECOND_SKILL, SKILL, agent, expect_error, item, observation,
    thread, user,
};
use crate::verification_models::{HistoryThread, ProviderObservation};
use serde_json::{Value, json};

fn native_reply() -> String {
    format!("{CANARY}::{ARGUMENTS}")
}

fn command() -> String {
    format!("/{SKILL} {ARGUMENTS}")
}

/// Original public events: user and assistant items as retained by the Router.
fn original(items: &[(&str, &str, &str)]) -> ProviderObservation {
    observation(
        items
            .iter()
            .map(|(id, kind, text)| item(id, kind, text))
            .collect(),
        "deadlineReached",
    )
}

fn view(items: Vec<Value>) -> HistoryThread {
    thread("thread", items)
}

fn single_invocation() -> ProviderObservation {
    original(&[
        ("user", "userMessage", &command()),
        ("reply", "agentMessage", &native_reply()),
    ])
}

fn canonical_view() -> HistoryThread {
    view(vec![
        user("user", &command()),
        agent("reply", &native_reply()),
    ])
}

#[test]
fn canonical_command_with_outer_trim_only_passes() {
    let setup = NativeSetup::new();
    let padded = format!("  {ARGUMENTS}  ");
    let trimmed_command = format!("/{SKILL} {ARGUMENTS}");
    let events = original(&[
        ("user", "userMessage", &trimmed_command),
        ("reply", "agentMessage", &native_reply()),
    ]);
    let views = [canonical_view(), canonical_view()];
    let trimmed =
        check_selected_history(&events, &views, "thread", &setup.fixture, &padded).unwrap();
    assert!(trimmed.trimmed_outer_whitespace);
    let same = check_selected_history(
        &single_invocation(),
        &views,
        "thread",
        &setup.fixture,
        ARGUMENTS,
    )
    .unwrap();
    assert!(!same.trimmed_outer_whitespace);
    assert_eq!(same.user_item_ids, ["user"]);
    assert_eq!(same.reply_item_id, "reply");
}

#[test]
fn changed_history_is_rejected() {
    let setup = NativeSetup::new();
    let wrong = [
        (
            view(vec![
                user("user", &format!("${SKILL} {ARGUMENTS}")),
                agent("reply", &native_reply()),
            ]),
            "changed",
        ),
        (
            view(vec![
                user("user", &format!("/{SECOND_SKILL} {ARGUMENTS}")),
                agent("reply", &native_reply()),
            ]),
            "changed",
        ),
        (
            view(vec![
                user("user", &format!("/{SKILL} literal-value café second-line")),
                agent("reply", &native_reply()),
            ]),
            "changed",
        ),
        (
            view(vec![user("user", &command()), agent("reply", "ORDINARY")]),
            "native effect",
        ),
        (
            thread(
                "other",
                vec![user("user", &command()), agent("reply", &native_reply())],
            ),
            "same thread",
        ),
    ];
    for (changed, fragment) in wrong {
        expect_error(
            check_selected_history(
                &single_invocation(),
                &[changed, canonical_view()],
                "thread",
                &setup.fixture,
                ARGUMENTS,
            ),
            fragment,
        );
    }
    assert!(
        check_selected_history(
            &single_invocation(),
            &[canonical_view()],
            "thread",
            &setup.fixture,
            ARGUMENTS
        )
        .is_err()
    );
}

#[test]
fn internal_argument_bytes_are_preserved() {
    let setup = NativeSetup::new();
    // `$`, UTF-8 and the internal newline each matter; only outer whitespace may trim.
    for corrupted in [
        ARGUMENTS.replace('$', ""),
        ARGUMENTS.replace("café", "cafe"),
        ARGUMENTS.replace('\n', " "),
    ] {
        let changed = view(vec![
            user("user", &format!("/{SKILL} {corrupted}")),
            agent("reply", &native_reply()),
        ]);
        expect_error(
            check_selected_history(
                &single_invocation(),
                &[canonical_view(), changed],
                "thread",
                &setup.fixture,
                ARGUMENTS,
            ),
            "changed the command or task of user item user",
        );
    }
}

#[test]
fn other_invocations_of_the_command_do_not_hide_the_selected_one() {
    // J3 invokes the same native command through Router with different arguments.
    let setup = NativeSetup::new();
    let events = original(&[
        (
            "router-user",
            "userMessage",
            &format!("/{SKILL} router-probe"),
        ),
        (
            "router-reply",
            "agentMessage",
            &format!("{CANARY}::router-probe"),
        ),
        ("user", "userMessage", &command()),
        ("reply", "agentMessage", &native_reply()),
    ]);
    let items = vec![
        user("router-user", &format!("/{SKILL} router-probe")),
        agent("router-reply", &format!("{CANARY}::router-probe")),
        user("user", &command()),
        agent("reply", &native_reply()),
    ];
    let shown = view(items.clone());
    let result = check_selected_history(
        &events,
        &[shown.clone(), shown.clone()],
        "thread",
        &setup.fixture,
        ARGUMENTS,
    )
    .unwrap();
    assert_eq!(result.user_item_ids, ["user"]);
    // The original same-ID duplicate counterexample: one identity shown twice.
    let mut repeated_items = items.clone();
    repeated_items.push(items[2].clone());
    expect_error(
        check_selected_history(
            &events,
            &[view(repeated_items), shown],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "repeated",
    );
}

/// An echoing provider: the submitted input and the native echo are separate facts
/// with equal text and distinct identities.
fn echoed_invocation() -> ProviderObservation {
    original(&[
        ("submitted", "userMessage", &command()),
        ("native-echo", "userMessage", &command()),
        ("reply", "agentMessage", &native_reply()),
    ])
}

fn echoed_view() -> HistoryThread {
    view(vec![
        user("submitted", &command()),
        user("native-echo", &command()),
        agent("reply", &native_reply()),
    ])
}

#[test]
fn equal_text_with_distinct_identities_both_survive() {
    let setup = NativeSetup::new();
    let result = check_selected_history(
        &echoed_invocation(),
        &[echoed_view(), echoed_view()],
        "thread",
        &setup.fixture,
        ARGUMENTS,
    )
    .unwrap();
    assert_eq!(result.user_item_ids, ["native-echo", "submitted"]);
}

#[test]
fn read_and_resume_must_show_the_original_identity_set() {
    let setup = NativeSetup::new();
    let lost = view(vec![
        user("submitted", &command()),
        agent("reply", &native_reply()),
    ]);
    expect_error(
        check_selected_history(
            &echoed_invocation(),
            &[lost, echoed_view()],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "lost user item native-echo",
    );
    let fabricated = view(vec![
        user("submitted", &command()),
        user("native-echo", &command()),
        user("invented", &command()),
        agent("reply", &native_reply()),
    ]);
    expect_error(
        check_selected_history(
            &echoed_invocation(),
            &[echoed_view(), fabricated],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "absent from the original events",
    );
    let corrupted = view(vec![
        user("submitted", &command()),
        user(
            "native-echo",
            &format!("/{SKILL} {}", ARGUMENTS.replace("café", "cafe")),
        ),
        agent("reply", &native_reply()),
    ]);
    expect_error(
        check_selected_history(
            &echoed_invocation(),
            &[echoed_view(), corrupted],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "changed the command or task of user item native-echo",
    );
}

#[test]
fn the_native_effect_keeps_its_original_identity_once() {
    let setup = NativeSetup::new();
    let renamed = view(vec![
        user("user", &command()),
        agent("other", &native_reply()),
    ]);
    expect_error(
        check_selected_history(
            &single_invocation(),
            &[canonical_view(), renamed],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "another identity",
    );
    let repeated = view(vec![
        user("user", &command()),
        agent("reply", &native_reply()),
        agent("reply-again", &native_reply()),
    ]);
    expect_error(
        check_selected_history(
            &single_invocation(),
            &[canonical_view(), repeated],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "repeated in history",
    );
    let no_effect = original(&[("user", "userMessage", &command())]);
    expect_error(
        check_selected_history(
            &no_effect,
            &[canonical_view(), canonical_view()],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "original events",
    );
}

#[test]
fn repeated_original_events_are_one_identity_and_altered_facts_are_not_blessed() {
    let setup = NativeSetup::new();
    // A native user group arrives in chunks: started, then updated to the full text.
    let chunked = observation(
        vec![
            item("native-echo", "userMessage", &format!("/{SKILL}")),
            json!({"kind": "itemUpdated", "item": {"itemId": "native-echo", "kind": {"kind": "userMessage"}, "text": command()}}),
            item("reply", "agentMessage", &native_reply()),
        ],
        "deadlineReached",
    );
    let shown = view(vec![
        user("native-echo", &command()),
        agent("reply", &native_reply()),
    ]);
    let result = check_selected_history(
        &chunked,
        &[shown.clone(), shown.clone()],
        "thread",
        &setup.fixture,
        ARGUMENTS,
    )
    .unwrap();
    assert_eq!(result.user_item_ids, ["native-echo"]);
    let twice = view(vec![
        user("native-echo", &command()),
        user("native-echo", &command()),
        agent("reply", &native_reply()),
    ]);
    expect_error(
        check_selected_history(
            &chunked,
            &[shown, twice],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "repeated user item native-echo",
    );
    // The original item's final text was altered; history showing the canonical text
    // under that identity is not an original fact.
    let altered = observation(
        vec![
            item("user", "userMessage", &command()),
            json!({"kind": "itemUpdated", "item": {"itemId": "user", "kind": {"kind": "userMessage"}, "text": "/other task"}}),
            item("reply", "agentMessage", &native_reply()),
        ],
        "deadlineReached",
    );
    expect_error(
        check_selected_history(
            &altered,
            &[canonical_view(), canonical_view()],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "original events retain no canonical",
    );
    let incomplete = observation(vec![], "resultLimitReached");
    expect_error(
        check_selected_history(
            &incomplete,
            &[canonical_view(), canonical_view()],
            "thread",
            &setup.fixture,
            ARGUMENTS,
        ),
        "incomplete",
    );
}
