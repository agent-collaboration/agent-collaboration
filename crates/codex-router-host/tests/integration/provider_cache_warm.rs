//! V29 (D1 R16): the Host keeps an idle Claude session's cache warm on its one
//! injected clock. Each case starts a real Host through startup composition
//! with a ManualClock and the Claude send-race fixture, which records every
//! frame the agent reads; time moves only through the Host's clock control.
//! Every case also requires zero session/cancel at the agent.

mod cache_warm_host;

use cache_warm_host::{
    CacheWarmHost, FIFTY_FIVE_MINUTES, ONE_MINUTE, SendEnd, carrying, method_positions, recorded,
    send_end, until, warm_prompt_positions, warm_prompts, warm_prompts_to,
};
use collaboration_protocol::{OperationId, ProviderOperationEffect, ProviderOperationStage};
use std::time::Duration;

/// Minutes on the Host's clock.
const fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

/// Oracle: V29 — "Host started through startup_composition with a
/// ManualClock; a real Claude-fixture session ends a turn; the clock advances
/// 55 minutes, then 55 more; then the Host shuts down. Expect two warm prompts
/// at the fixture, none after shutdown. ... Zero session/cancel calls
/// throughout." Shutdown releases the Host's clock. Advancing it afterward
/// sends nothing to the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cache_warm_runs_through_host_composition() {
    // Arrange: a real turn ends on a session created after startup.
    let host = CacheWarmHost::start(&[]).await;
    host.release("release");
    let session = host.create().await;
    let real = host.prompt(&session, "real work").await;
    assert_eq!(send_end(&real), SendEnd::Prompted, "{real:?}");
    host.until_warm_timer_armed().await;
    let before_any_step = warm_prompts(&host.receipt);

    // Act: 55 minutes, then 55 more, then shutdown with a timer armed.
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&session, 1).await;
    host.until_warm_timer_armed().await;
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&session, 2).await;
    host.until_warm_timer_armed().await;
    let stopped = host.shutdown().await;
    stopped.advance_after_stop(FIFTY_FIVE_MINUTES * 2);

    // Assert
    stopped.assert_stopped();
    assert_eq!(before_any_step, 0, "nothing before the clock moves");
    assert_eq!(
        warm_prompts_to(&stopped.receipt, &session),
        2,
        "two warm prompts, and none once the Host has shut down"
    );
    assert_eq!(
        warm_prompts(&stopped.receipt),
        2,
        "no warm prompt elsewhere"
    );
    assert_eq!(recorded(&stopped.receipt, "session/cancel"), 0, "no cancel");
}

/// Oracle: V29 — "a Claude session created after startup arms itself". The
/// Host runs two hours with no session; a session created then gets its warm
/// prompt 55 minutes after its real turn, and a second session created after
/// that warm prompt arms itself too. Each warm prompt names its own session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_created_after_startup_arms_itself() {
    // Arrange: the Host has run two hours with no session.
    let host = CacheWarmHost::start(&[]).await;
    host.release("release");
    host.advance(minutes(120));

    // Act: the first session is created, works, and is warmed.
    let first = host.create().await;
    let first_turn = host.prompt(&first, "first real work").await;
    host.until_warm_timer_armed().await;
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&first, 1).await;
    host.until_warm_timer_armed().await;
    // Act: a second session is created after that warm prompt.
    let second = host.create().await;
    host.advance(ONE_MINUTE);
    let second_turn = host.prompt(&second, "second real work").await;
    host.until_warm_timer_armed().await;
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&second, 1).await;
    host.until_warm_prompts_to(&first, 2).await;
    let stopped = host.shutdown().await;
    let receipt = &stopped.receipt;

    // Assert
    stopped.assert_stopped();
    assert_eq!(send_end(&first_turn), SendEnd::Prompted, "{first_turn:?}");
    assert_eq!(send_end(&second_turn), SendEnd::Prompted, "{second_turn:?}");
    assert_ne!(first, second);
    assert_eq!(warm_prompts_to(receipt, &first), 2);
    assert_eq!(
        warm_prompts_to(receipt, &second),
        1,
        "the later session armed itself"
    );
    assert_eq!(
        warm_prompts(receipt),
        3,
        "every warm prompt names its session"
    );
    assert_eq!(recorded(receipt, "session/cancel"), 0, "no cancel");
}

/// Oracle: V29 — "a real message steered into a warm turn resets the 8-hour
/// limit". The warm turn at 55 minutes is held while the owner's message is
/// steered into it, and ends at 7h30. A warm prompt is then due at 8h25: past
/// 8 hours from the first real turn, so it would be suppressed if the old limit
/// held, and it is sent. At 16h the limit counted from 7h30 has passed, so the
/// next due prompt is suppressed: warm turns do not extend it, and it stops. A
/// session created then, whose warm prompt is due 55 minutes later, shows the
/// suppressed one was decided before that.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steered_real_message_restarts_the_keep_warm_limit() {
    // Arrange: every prompt ends at once except the second, the first warm prompt.
    let host = CacheWarmHost::start(&["hold-each-prompt"]).await;
    host.release_prompts_except(&[2], 16);
    let session = host.create().await;
    let real = host.prompt(&session, "real work").await;
    host.until_warm_timer_armed().await;
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&session, 1).await;

    // Act: a real message is steered into the running warm turn.
    let steered = host.prompt(&session, "steered real work").await;
    host.advance(minutes(395));
    let warm_while_turn_runs = warm_prompts_to(&host.receipt, &session);
    host.release("release-2");
    host.until_warm_timer_armed().await;
    // Act: 8h25 from the first real turn, 55 minutes after the steered turn ended.
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&session, 2).await;
    host.until_warm_timer_armed().await;
    // Act: 16h, past 8 hours since the steered turn ended at 7h30.
    host.advance(minutes(455));
    let witness = host.create().await;
    let witness_turn = host.prompt(&witness, "witness work").await;
    host.until_warm_timer_armed().await;
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&witness, 1).await;
    let stopped = host.shutdown().await;
    let receipt = &stopped.receipt;

    // Assert
    stopped.assert_stopped();
    assert_eq!(send_end(&real), SendEnd::Prompted, "{real:?}");
    assert_eq!(
        send_end(&steered),
        SendEnd::Steered,
        "the owner's message joins the warm turn: {steered:?}"
    );
    assert_eq!(warm_while_turn_runs, 1, "no warm prompt while a turn runs");
    assert_eq!(
        carrying(receipt, "_session/steering", "steered real work"),
        1
    );
    assert_eq!(
        carrying(receipt, "session/prompt", "steered real work"),
        0,
        "the steered message is not also prompted"
    );
    assert_eq!(
        warm_prompts_to(receipt, &session),
        2,
        "the 8h25 prompt is sent past the old limit; the 16h one is not"
    );
    assert_eq!(send_end(&witness_turn), SendEnd::Prompted);
    assert_eq!(warm_prompts_to(receipt, &witness), 1);
    assert_eq!(recorded(receipt, "session/cancel"), 0, "no cancel");
}

/// Oracle: V29 — "replayed history doesn't count". A session loaded with
/// session/load, whose agent replays an old turn, is not armed by that turn:
/// a session whose real turn ended at the same instant is warmed 55 minutes
/// later and the loaded one is not. Its next real turn arms it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replayed_history_neither_arms_nor_counts() {
    // Arrange: a created session, and a loaded one whose old turn is replayed.
    let host = CacheWarmHost::start(&["replay-on-load"]).await;
    host.release("release");
    let witness = host.create().await;
    let (loaded, load) = host.load("replayed-session").await;
    let witness_turn = host.prompt(&witness, "witness work").await;
    host.until_warm_timer_armed().await;

    // Act: 55 minutes after the replay and the witness's real turn.
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&witness, 1).await;
    host.until_warm_timer_armed().await;
    let loaded_after_replay = warm_prompts_to(&host.receipt, &loaded);
    // Act: the loaded session's first real turn.
    host.advance(ONE_MINUTE);
    let loaded_turn = host.prompt(&loaded, "real work after load").await;
    host.until_warm_timer_armed().await;
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&loaded, 1).await;
    host.until_warm_prompts_to(&witness, 2).await;
    let stopped = host.shutdown().await;
    let receipt = &stopped.receipt;

    // Assert
    stopped.assert_stopped();
    assert!(
        matches!(
            &load,
            Ok(collaboration_client::ConversationOperationResult::Completed { .. })
        ),
        "the session loads: {load:?}"
    );
    assert_eq!(method_positions(receipt, "session/load", &loaded).len(), 1);
    assert_eq!(send_end(&witness_turn), SendEnd::Prompted);
    assert_eq!(send_end(&loaded_turn), SendEnd::Prompted, "{loaded_turn:?}");
    assert_eq!(
        loaded_after_replay, 0,
        "the replayed turn did not arm the loaded session"
    );
    assert_eq!(
        warm_prompts_to(receipt, &loaded),
        1,
        "one warm prompt, after its real turn"
    );
    let real_prompt = method_positions(receipt, "session/prompt", &loaded)
        .first()
        .copied()
        .expect("the loaded session's real prompt");
    assert!(
        warm_prompt_positions(receipt, &loaded)
            .iter()
            .all(|warm| *warm > real_prompt),
        "the warm prompt follows the real turn"
    );
    assert_eq!(warm_prompts_to(receipt, &witness), 2);
    assert_eq!(recorded(receipt, "session/cancel"), 0, "no cancel");
}

/// Oracle: V29 — "a session with settings held or an unknown earlier outcome
/// gets no warm prompt". One session's effort change is answered without the
/// change (its settings are held); another's steer is answered with an
/// outcome nobody knows (its outcome stays unknown). Both ended a real turn,
/// as did a third whose warm prompt, due in the same clock step, is sent.
/// Once the held setting is confirmed, that session's next real turn arms its
/// timer again and it is warmed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn held_settings_and_unknown_outcome_get_no_warm_prompt() {
    // Arrange: an unknown steer outcome on the first session.
    let host =
        CacheWarmHost::start(&["first-steer-unknown", "first-effort-change-unconfirmed"]).await;
    let unknown = host.create().await;
    let held = host.create().await;
    let witness = host.create().await;
    let running = host.spawn_prompt(&unknown, &OperationId::generate(), "first");
    until(|| recorded(&host.receipt, "session/prompt") == 1).await;
    let unknown_steer = OperationId::generate();
    let unknown_answer = host
        .spawn_prompt(&unknown, &unknown_steer, "second")
        .await
        .expect("steer task");
    let unknown_snapshot = host.show(&unknown_steer).await;
    let running = running.await.expect("first prompt task");
    host.until_warm_timer_armed().await;
    // Arrange: a real turn, then an effort change the agent does not confirm.
    host.advance(ONE_MINUTE);
    let held_turn = host.prompt(&held, "held real work").await;
    host.until_warm_timer_armed().await;
    let unconfirmed = host.set_effort(&held, "high").await;
    // Arrange: the witness's real turn.
    host.advance(ONE_MINUTE);
    let witness_turn = host.prompt(&witness, "witness work").await;
    host.until_warm_timer_armed().await;

    // Act: all three warm prompts fall due in one step.
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&witness, 1).await;
    // The witness's timer runs independently. Keep the effort hold in place
    // until the held session's real prompt and first warm attempt have settled.
    host.until_terminal_prompts_to(&held, 2).await;
    host.until_warm_timer_armed().await;
    let held_while_held = warm_prompts_to(&host.receipt, &held);
    let unknown_after_due = warm_prompts_to(&host.receipt, &unknown);
    // Act: the setting is confirmed, then the held session's next real turn.
    let confirmed = host.set_effort(&held, "high").await;
    host.advance(ONE_MINUTE);
    let held_again = host.prompt(&held, "held real work again").await;
    host.until_warm_timer_armed().await;
    host.advance(FIFTY_FIVE_MINUTES);
    host.until_warm_prompts_to(&held, 1).await;
    host.until_warm_prompts_to(&witness, 2).await;
    let stopped = host.shutdown().await;
    let receipt = &stopped.receipt;

    // Assert: the preconditions the fixture set up.
    stopped.assert_stopped();
    assert_eq!(send_end(&running), SendEnd::Prompted, "{running:?}");
    assert!(
        !matches!(
            send_end(&unknown_answer),
            SendEnd::Prompted | SendEnd::Steered
        ),
        "{unknown_answer:?}"
    );
    assert!(
        unknown_snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.stage == ProviderOperationStage::Terminal
                && snapshot.effect == ProviderOperationEffect::Unknown
        }),
        "the steer's outcome stays unknown: {unknown_snapshot:?}"
    );
    assert_eq!(send_end(&held_turn), SendEnd::Prompted, "{held_turn:?}");
    assert!(
        unconfirmed.is_err(),
        "an unconfirmed change holds: {unconfirmed:?}"
    );
    assert!(confirmed.is_ok(), "a confirmed change: {confirmed:?}");
    assert_eq!(send_end(&witness_turn), SendEnd::Prompted);
    assert_eq!(send_end(&held_again), SendEnd::Prompted, "{held_again:?}");
    // Assert: no warm prompt to a held or uncertain session.
    assert_eq!(held_while_held, 0, "settings held: no warm prompt");
    assert_eq!(
        unknown_after_due, 0,
        "unknown earlier outcome: no warm prompt"
    );
    assert_eq!(
        warm_prompts_to(receipt, &unknown),
        0,
        "unknown earlier outcome: never warmed"
    );
    assert_eq!(
        warm_prompts_to(receipt, &held),
        1,
        "re-armed by the next real turn"
    );
    let confirmed_change = method_positions(receipt, "session/set_config_option", &held)
        .last()
        .copied()
        .expect("the confirming change");
    assert!(
        warm_prompt_positions(receipt, &held)
            .iter()
            .all(|warm| *warm > confirmed_change),
        "the held session is warmed only after its setting is confirmed"
    );
    assert_eq!(warm_prompts_to(receipt, &witness), 2);
    assert_eq!(recorded(receipt, "session/cancel"), 0, "no cancel");
}
