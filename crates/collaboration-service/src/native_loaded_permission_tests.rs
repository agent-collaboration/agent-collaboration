//! Known-route input reports permission differences while preserving attachment checks.
use super::*;

#[path = "native_loaded_permission_fixture.rs"]
mod policy_fixture;
use policy_fixture::*;

#[path = "native_policy_preservation_tests.rs"]
mod preservation_policy_tests;
#[path = "native_recorded_classification_tests.rs"]
mod recorded_classification_tests;

#[tokio::test]
async fn a_message_without_a_client_id_gets_a_uuidv7_correlation() {
    let fixture = RecordedPolicyFixture::create().await.unwrap();
    let server = policy_server(
        &fixture,
        PolicyServerCase {
            cold: ReplyPolicy::Matching,
            loaded: ReplyPolicy::Matching,
            loaded_delivery: MessageDelivery::Auto,
        },
    )
    .unwrap();
    let broker: Arc<dyn ApprovalBroker> = fixture.broker.clone();
    let weak = Arc::downgrade(&broker);
    let mut minted = Vec::new();
    for _ in 0..2 {
        let mut request = fixture.message_request(&weak);
        request.params.client_user_message_id = None;
        let outcome = tokio::time::timeout(Duration::from_secs(5), dispatch_message(request))
            .await
            .unwrap();
        let NativeMessageOutcome::Accepted(receipt) = outcome else {
            panic!("a matching policy accepts the message");
        };
        minted.push(String::from(receipt.client_user_message_id));
    }
    let exchanges = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    for (correlation, requests) in minted.iter().zip(&exchanges) {
        assert_eq!(
            uuid::Uuid::parse_str(correlation)
                .unwrap()
                .get_version_num(),
            7,
            "minted correlation {correlation}"
        );
        assert!(
            requests
                .iter()
                .any(|request| request["method"] == "turn/start"
                    && request["params"]["clientUserMessageId"] == correlation.as_str()),
            "the native input carries the minted correlation"
        );
    }
}

#[tokio::test]
async fn cold_and_loaded_messages_both_report_narrowed_permissions() {
    let fixture = RecordedPolicyFixture::create().await.unwrap();
    let server = policy_server(
        &fixture,
        PolicyServerCase {
            cold: ReplyPolicy::Narrowed,
            loaded: ReplyPolicy::Narrowed,
            loaded_delivery: MessageDelivery::Auto,
        },
    )
    .unwrap();
    let broker: Arc<dyn ApprovalBroker> = fixture.broker.clone();
    let weak = Arc::downgrade(&broker);
    let first = tokio::time::timeout(
        Duration::from_secs(5),
        dispatch_message(fixture.message_request(&weak)),
    )
    .await
    .unwrap();
    let second = tokio::time::timeout(
        Duration::from_secs(5),
        dispatch_message(fixture.message_request(&weak)),
    )
    .await
    .unwrap();
    let exchanges = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    for outcome in [first, second] {
        let NativeMessageOutcome::Accepted(receipt) = outcome else {
            panic!("permission difference refused");
        };
        assert_eq!(
            receipt.permission_differences,
            vec![collaboration_protocol::PermissionDifference::WritableRoots]
        );
        assert_eq!(
            receipt.resume_effect,
            collaboration_protocol::AcceptedResumeEffect::Accepted
        );
    }
    assert!(exchanges.iter().all(|requests| {
        requests
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count()
            == 1
    }));
    let second_resume = exchanges[1]
        .iter()
        .find(|request| request["method"] == "thread/resume")
        .expect("loaded policy snapshot must be requested");
    assert_eq!(
        second_resume["params"],
        json!({"threadId":THREAD,"excludeTurns":true}),
        "loaded validation must not override config or trigger upstream cache rebuild"
    );
}

#[tokio::test]
async fn matching_loaded_policy_allows_each_message_mode_without_overrides() {
    for mode in [
        MessageDelivery::Auto,
        MessageDelivery::Queue,
        MessageDelivery::Steer,
    ] {
        let fixture = RecordedPolicyFixture::create().await.unwrap();
        let server = policy_server(
            &fixture,
            PolicyServerCase {
                cold: ReplyPolicy::Matching,
                loaded: ReplyPolicy::Matching,
                loaded_delivery: mode,
            },
        )
        .unwrap();
        let broker: Arc<dyn ApprovalBroker> = fixture.broker.clone();
        let weak = Arc::downgrade(&broker);
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            dispatch_message(fixture.message_request(&weak)),
        )
        .await
        .unwrap();
        let mut request = fixture.message_request(&weak);
        request.params.delivery = mode;
        let second = tokio::time::timeout(Duration::from_secs(5), dispatch_message(request))
            .await
            .unwrap();
        let exchanges = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first, NativeMessageOutcome::Accepted(_)));
        let NativeMessageOutcome::Accepted(receipt) = second else {
            panic!("matching loaded policy refused");
        };
        assert_eq!(
            receipt.resume_effect,
            collaboration_protocol::AcceptedResumeEffect::Accepted
        );
        let loaded = &exchanges[1];
        assert_eq!(
            loaded
                .iter()
                .find(|call| call["method"] == "thread/resume")
                .unwrap()["params"],
            json!({"threadId":THREAD,"excludeTurns":true})
        );
        let input_method = match mode {
            MessageDelivery::Auto => "turn/start",
            MessageDelivery::Queue => "thread/queue/add",
            MessageDelivery::Steer => "turn/steer",
        };
        assert!(loaded.iter().any(|call| call["method"] == input_method));
    }
}

#[tokio::test]
async fn narrowed_loaded_policy_reports_differences_for_queue_and_steer() {
    for mode in [MessageDelivery::Queue, MessageDelivery::Steer] {
        let fixture = RecordedPolicyFixture::create().await.unwrap();
        let server = policy_server(
            &fixture,
            PolicyServerCase {
                cold: ReplyPolicy::Matching,
                loaded: ReplyPolicy::Narrowed,
                loaded_delivery: mode,
            },
        )
        .unwrap();
        let broker: Arc<dyn ApprovalBroker> = fixture.broker.clone();
        let weak = Arc::downgrade(&broker);
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            dispatch_message(fixture.message_request(&weak)),
        )
        .await
        .unwrap();
        let mut request = fixture.message_request(&weak);
        request.params.delivery = mode;
        let second = tokio::time::timeout(Duration::from_secs(5), dispatch_message(request))
            .await
            .unwrap();
        let exchanges = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first, NativeMessageOutcome::Accepted(_)));
        let NativeMessageOutcome::Accepted(receipt) = second else {
            panic!("narrowed loaded input refused");
        };
        assert_eq!(
            receipt.resume_effect,
            collaboration_protocol::AcceptedResumeEffect::Accepted
        );
        assert_eq!(
            receipt.permission_differences,
            vec![collaboration_protocol::PermissionDifference::WritableRoots]
        );
        assert_eq!(
            exchanges[1]
                .iter()
                .filter(|call| matches!(
                    call["method"].as_str(),
                    Some("turn/start" | "turn/steer" | "thread/queue/add")
                ))
                .count(),
            1
        );
    }
}
