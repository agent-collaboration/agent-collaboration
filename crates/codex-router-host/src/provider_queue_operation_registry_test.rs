use super::*;
use std::sync::{Arc, Barrier};

fn fixture() -> (SessionRef, ProviderBindingIdentity) {
    let target: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"cursor-local"},
        "sessionId":"provider-session"
    }))
    .expect("target");
    let binding = serde_json::from_value(serde_json::json!({
            "endpoint":target.endpoint,"bindingId":"binding",
            "runtime":{"provider":"cursor","runtimeName":"fixture"},
            "transport":"stdioAcp","generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":1},
            "capabilities":[{"name":"prompt","status":"supported","evidence":"advertised"}]
        })).expect("binding");
    (target, binding)
}

#[test]
fn list_keeps_admission_order_and_cancel_excludes_only_one_input() {
    let registry = ProviderQueueOperationRegistry::default();
    let (target, binding) = fixture();
    let first = (
        OperationId::generate(),
        session_event_model::InputId::generate(),
    );
    let second = (
        OperationId::generate(),
        session_event_model::InputId::generate(),
    );
    let first_contents =
        [session_event_model::PromptContent::text("first\n  item".into()).expect("text")];
    registry.record_queued_contents(
        first.0.clone(),
        target.clone(),
        binding.clone(),
        first.1.clone(),
        &first_contents,
    );
    let second_contents =
        [session_event_model::PromptContent::text("second item".into()).expect("text")];
    registry.record_queued_contents(
        second.0.clone(),
        target.clone(),
        binding,
        second.1.clone(),
        &second_contents,
    );
    let listed = registry.list(&target);
    assert_eq!(
        listed
            .iter()
            .map(|item| (&item.input_id, item.position))
            .collect::<Vec<_>>(),
        vec![(&first.1, 1), (&second.1, 2)]
    );
    assert_eq!(listed[0].preview, "first item");
    registry.cancel(&target, &first.1).expect("cancel first");
    assert_eq!(
        registry.cancel(&target, &first.1),
        Err(ProviderQueueCancellationError::NotQueued)
    );
    assert!(!registry.try_start(&first.0));
    assert_eq!(registry.list(&target)[0].input_id, second.1);
}

#[test]
fn content_preview_uses_first_text_block_after_non_text_content() {
    let registry = ProviderQueueOperationRegistry::default();
    let (target, binding) = fixture();
    let input_id = session_event_model::InputId::generate();
    let contents = vec![
        session_event_model::PromptContent::resource_link(
            "https://example.test/context".into(),
            "context".into(),
            None,
        )
        .expect("resource link"),
        session_event_model::PromptContent::text("  first\n  text\tblock  ".into()).expect("text"),
        session_event_model::PromptContent::text("second text".into()).expect("text"),
    ];
    registry.record_queued_contents(
        OperationId::generate(),
        target.clone(),
        binding,
        input_id,
        &contents,
    );
    assert_eq!(registry.list(&target)[0].preview, "first text block");
}

#[test]
fn cancel_and_start_race_has_one_winner() {
    let registry = Arc::new(ProviderQueueOperationRegistry::default());
    let (target, binding) = fixture();
    let operation_id = OperationId::generate();
    let input_id = session_event_model::InputId::generate();
    let contents = [session_event_model::PromptContent::text("racing input".into()).expect("text")];
    registry.record_queued_contents(
        operation_id.clone(),
        target.clone(),
        binding,
        input_id.clone(),
        &contents,
    );
    let barrier = Arc::new(Barrier::new(3));
    let started = {
        let registry = Arc::clone(&registry);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            registry.try_start(&operation_id)
        })
    };
    let cancelled = {
        let registry = Arc::clone(&registry);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            registry.cancel(&target, &input_id).is_ok()
        })
    };
    barrier.wait();
    assert_ne!(
        started.join().expect("start result"),
        cancelled.join().expect("cancel result")
    );
}
