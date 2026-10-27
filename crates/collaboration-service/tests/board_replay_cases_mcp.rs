#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::match_wildcard_for_single_variants,
    reason = "lint pass follows the crate split"
)]
//! V6's extracted histories run through the real MCP service and captured native stores.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "board_replay_support/replay_assertions.rs"]
mod replay_assertions;
#[path = "board_replay_support/replay_case.rs"]
mod replay_case;
#[path = "board_replay_support/replay_driver.rs"]
mod replay_driver;

use board_fixture::TestError;
use replay_case::ReplayCase;
use replay_driver::ReplayBoard;

const MESSAGING: &str = include_str!("fixtures/board_replay_cases/messaging-first-112.json");
const REPLACEMENT: &str =
    include_str!("fixtures/board_replay_cases/lead-replacement-board-design.json");
const LATE_FACTS: &str = include_str!("fixtures/board_replay_cases/close-with-late-fact.json");

#[tokio::test]
async fn messaging_history_answers_the_three_questions() -> Result<(), TestError> {
    replay_history(MESSAGING, 41).await
}
#[tokio::test]
async fn lead_replacement_history_answers_before_and_after_succession() -> Result<(), TestError> {
    replay_history(REPLACEMENT, 40).await
}
#[tokio::test]
async fn closed_work_keeps_late_evidence_and_conversation_answers() -> Result<(), TestError> {
    replay_history(LATE_FACTS, 36).await
}
#[allow(clippy::panic_in_result_fn)]
async fn replay_history(source: &str, expected_events: usize) -> Result<(), TestError> {
    let case: ReplayCase = serde_json::from_str(source)?;
    case.validate_metadata()?;
    assert_eq!(case.events.len(), expected_events);
    let mut board = ReplayBoard::start(&case).await?;
    for event in &case.events {
        board.replay(event).await?;
        for question in case
            .questions
            .iter()
            .filter(|question| question.as_of == Some(event.seq))
        {
            board.assert_question(&case, question).await?;
        }
    }
    for question in case
        .questions
        .iter()
        .filter(|question| question.as_of.is_none())
    {
        board.assert_question(&case, question).await?;
    }
    eprintln!(
        "{}: {} real public events; {} question checkpoints",
        case.case,
        case.events.len(),
        case.questions.len()
    );
    board.stop().await
}

#[tokio::test]
async fn replaced_leads_and_used_grants_keep_their_refusals() -> Result<(), TestError> {
    replay_probes(REPLACEMENT, 2).await
}
#[tokio::test]
async fn closed_work_refuses_unanchored_facts_and_new_owner_rows() -> Result<(), TestError> {
    replay_probes(LATE_FACTS, 3).await
}
#[allow(clippy::panic_in_result_fn)]
async fn replay_probes(source: &str, expected_probes: usize) -> Result<(), TestError> {
    let case: ReplayCase = serde_json::from_str(source)?;
    case.validate_metadata()?;
    assert_eq!(case.probes.len(), expected_probes);
    for probe in &case.probes {
        let mut board = ReplayBoard::start(&case).await?;
        for event in case.events.iter().filter(|event| event.seq <= probe.after) {
            board.replay(event).await?;
        }
        let args = board.resolve(&probe.args, "")?;
        let (reply, _) = board
            .call_operation(&probe.actor, None, &probe.op, args)
            .await?;
        assert_eq!(
            probe
                .expect
                .get("decision")
                .and_then(serde_json::Value::as_str),
            Some("rejected")
        );
        assert!(
            matches!(
                reply.result,
                message_board::public_replies::RequestResult::Decided(
                    message_board::public_replies::RequestDecision::Rejected { .. }
                )
            ),
            "{}: {:?}",
            probe.note,
            reply.result
        );
        if let Some(reason) = probe
            .expect
            .get("reason")
            .and_then(serde_json::Value::as_str)
        {
            let response = serde_json::json!({"result":reply});
            assert_eq!(board_fixture::refusal_kind(&response), Some(reason));
        }
        eprintln!(
            "{} after {}: {} -> rejected",
            case.case, probe.after, probe.op
        );
        board.stop().await?;
    }
    Ok(())
}
