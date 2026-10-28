//! History faults use the actual owner while channel receipts keep an independent observer.
use super::super::native_history_fault_fixture::{
    IndependentNativeHistoryRead, NativeHistoryFaults,
};
use super::*;

#[tokio::test]
async fn question_answer_delivery_reports_later_history_write_failure() {
    let (broker, _, _) = super::super::tests::fixture_broker().await;
    let (requester, approver) = typed_participants(&broker);
    let receiver = broker
        .request_question(
            requester,
            approver.clone(),
            typed_question("write-failure-question"),
            None,
        )
        .await
        .expect("question admitted");
    let answer = QuestionResponse::Answered {
        content: serde_json::from_value(serde_json::json!({"yes": true})).expect("answer"),
    };
    let (recorded, validation_complete) = tokio::sync::oneshot::channel();
    let (resume, proceed) = tokio::sync::oneshot::channel();
    *broker.question_before_send.lock().await = Some(TypedAdmissionPause {
        recorded,
        resume: proceed,
    });
    let answering_broker = broker.clone();
    let delivered_answer = answer.clone();
    let answering = tokio::spawn(async move {
        answering_broker
            .respond_question("write-failure-question", &approver, delivered_answer)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), validation_complete)
        .await
        .expect("bounded validation wait")
        .expect("validation reached pause");
    let fault = NativeHistoryFaults::new(&broker);
    let before_revision = fault
        .integer("SELECT revision FROM interaction_history_revision")
        .await
        .expect("native revision before fault");
    // Both changes commit in one borrowed native transaction, after validation.
    fault.execute_all(&[
        "UPDATE typed_interaction_history SET record_json='{}' WHERE request_id='write-failure-question'",
        "UPDATE interaction_history_revision SET revision=revision+1",
    ])
        .await.expect("native corruption and revision committed together");
    assert_eq!(fault.text("SELECT record_json FROM typed_interaction_history WHERE request_id='write-failure-question'")
        .await.expect("physical native corruption"), "{}");
    assert_eq!(
        fault
            .integer("SELECT revision FROM interaction_history_revision")
            .await
            .expect("physical native revision"),
        before_revision + 1
    );
    resume.send(()).expect("permit answer delivery");
    assert!(matches!(
        answering.await.expect("answer task"),
        Err(InteractionHistoryError::Unavailable)
    ));
    assert_eq!(receiver.await.expect("response was delivered"), answer);
    assert!(
        !broker
            .pending_questions
            .lock()
            .await
            .contains_key("write-failure-question")
    );
    let record = broker
        .interaction_history
        .interaction("write-failure-question")
        .await
        .expect("history");
    assert!(matches!(
        record,
        InteractionHistoryRecord::Question {
            state: QuestionHistoryState::Pending,
            ..
        }
    ));
}

#[tokio::test]
async fn approval_decision_native_write_failure_keeps_channel_unsent_then_commit_precedes_delivery()
{
    let (broker, _, directory) = super::super::tests::fixture_broker().await;
    let (requester, approver) = typed_participants(&broker);
    let mut receiver = broker
        .request_typed_approval(
            requester,
            approver.clone(),
            typed_request("approval-write-failure"),
            CancellationToken::new(),
            CancellationToken::new(),
            None,
        )
        .await
        .expect("pending approval");
    let mut independent = IndependentNativeHistoryRead::open(&directory.join("interaction.sqlite"))
        .await
        .expect("independent native observer");
    let fault = NativeHistoryFaults::new(&broker);
    fault.execute_all(&["CREATE TRIGGER reject_history_update BEFORE UPDATE ON typed_interaction_history BEGIN SELECT RAISE(ABORT, 'fixture failed decision'); END"])
        .await.expect("native write rejection installed");
    assert_eq!(fault.integer("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name='reject_history_update'")
        .await.expect("native trigger readback"), 1);
    let decide = || TypedInteractionDecision::SelectApproval {
        option_id: "allow-once".into(),
        acknowledge_persistent: false,
        note: None,
    };
    assert!(matches!(
        broker
            .decide_typed_interaction("approval-write-failure", &approver, decide())
            .await,
        Err(InteractionHistoryError::Unavailable)
    ));
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    let record_json = independent
        .record_json("approval-write-failure")
        .await
        .expect("unchanged committed row");
    let record: InteractionHistoryRecord = serde_json::from_str(&record_json).expect("record");
    assert_eq!(
        record.approval_state(),
        Some(&InteractionHistoryState::Pending)
    );
    let native_pending = fault.text("SELECT record_json FROM typed_interaction_history WHERE request_id='approval-write-failure'")
        .await.expect("unchanged native row after rejected write");
    assert_eq!(native_pending, record_json);
    fault
        .execute_all(&["DROP TRIGGER reject_history_update"])
        .await
        .expect("remove native fault");
    assert_eq!(fault.integer("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name='reject_history_update'")
        .await.expect("native fault removed"), 0);
    // This observer deliberately does not borrow the serialized owner: waiting
    // for its mutex could hide a premature channel send before the commit.
    let consumer = tokio::spawn(async move {
        let outcome = receiver.await.expect("approval delivered after commit");
        let committed = independent
            .record_json("approval-write-failure")
            .await
            .expect("decision already durable at channel receipt");
        let record: InteractionHistoryRecord =
            serde_json::from_str(&committed).expect("committed record");
        assert!(
            matches!(record.approval_state(),Some(InteractionHistoryState::Decided {option_id}) if option_id.as_str()=="allow-once")
        );
        independent.close().await.expect("close native observer");
        outcome
    });
    broker
        .decide_typed_interaction("approval-write-failure", &approver, decide())
        .await
        .expect("decision");
    assert!(matches!(
        consumer.await.expect("consumer"),
        TypedApprovalResolution::Selected(_)
    ));
}
