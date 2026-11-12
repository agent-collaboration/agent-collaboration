//! Each family's typed failure maps to a spec 1 §5 rejection reason only where it is that same
//! reason, and its wire payload keeps today's `kind`. The oracle is the spec 1 §5 table and the
//! published `-32050` payload kinds, never the mapping code itself.
use super::{CollaborationRejection, CollaborationRejectionReason as Reason};
use crate::collaboration_application::{
    ConversationFailure, INVALID_NATIVE_PARAMETERS, InteractionFailure, JournalFailure,
    JournalUnavailableKind, NativeSessionFailure, NativeSessionFailureKind, NativeSessionStage,
    ObservationFailure, ProviderInventoryFailure, ProviderInventoryFailureKind, QuestionRejection,
};
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, NonEmptyText, ProviderInspectFailure,
    ProviderInspectFailureKind, ProviderOperationEffect, ProviderSettingsFailure,
    ProviderSettingsFailureKind,
};
use serde_json::{Value, json};

fn wire_kind(failure: &impl serde::Serialize) -> String {
    let payload = serde_json::to_value(failure).expect("failure payload encodes");
    payload
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .expect("failure payload names its kind")
}

#[test]
fn every_reason_carries_its_spec_name() {
    // Arrange: the spec 1 §5 table.
    let table = [
        (Reason::Overloaded, "overloaded"),
        (Reason::Archived, "archived"),
        (Reason::InvalidShape, "invalidShape"),
        (Reason::CursorInvalid, "cursorInvalid"),
        (Reason::StaleRevision, "staleRevision"),
        (Reason::ConflictingRequest, "conflictingRequest"),
    ];

    // Act & assert.
    for (reason, spec_name) in table {
        assert_eq!(reason.spec_name(), spec_name);
    }
}

#[test]
fn conversation_failures_map_only_overload() {
    // Arrange.
    let operation = |kind| {
        ConversationFailure::Operation(ConversationOperationFailure {
            kind,
            stage: ConversationOperationFailureStage::Binding,
            effect: ProviderOperationEffect::None,
            message: NonEmptyText::try_from("failure".to_owned()).expect("message text"),
            operation_id: None,
            invalid_setting: None,
            provider_code: None,
            target: None,
            endpoint: None,
            availability: None,
        })
    };
    let settings = |kind| ProviderSettingsFailure {
        kind,
        stage: None,
        target: None,
        message: "failure".into(),
        setting: None,
        value: None,
        advertised: Vec::new(),
    };
    let inspect = |kind| ProviderInspectFailure {
        kind,
        stage: None,
        target: None,
        message: "failure".into(),
    };

    // Act & assert.
    assert_eq!(
        operation(ConversationOperationFailureKind::Overloaded).rejection_reason(),
        Some(Reason::Overloaded)
    );
    assert_eq!(
        operation(ConversationOperationFailureKind::StaleGeneration).rejection_reason(),
        None
    );
    assert_eq!(
        operation(ConversationOperationFailureKind::InvalidRequest).rejection_reason(),
        None
    );
    assert_eq!(
        ConversationFailure::InvalidStoredOperation.rejection_reason(),
        None
    );
    assert_eq!(
        settings(ProviderSettingsFailureKind::Overloaded).rejection_reason(),
        Some(Reason::Overloaded)
    );
    assert_eq!(
        settings(ProviderSettingsFailureKind::Busy).rejection_reason(),
        None
    );
    assert_eq!(
        inspect(ProviderInspectFailureKind::Overloaded).rejection_reason(),
        Some(Reason::Overloaded)
    );
    assert_eq!(
        inspect(ProviderInspectFailureKind::NotFound).rejection_reason(),
        None
    );
}

#[test]
fn interaction_and_observation_payloads_keep_their_kinds() {
    // Interactions are family-specific.
    let answer = InteractionFailure::QuestionRejected(QuestionRejection::InvalidAnswer {
        field_id: "choice".into(),
    });
    assert_eq!(answer.rejection_reason(), None);
    assert_eq!(
        answer.payload(),
        json!({"kind":"invalidAnswer","stage":"inspect","message":"Question response rejected","fieldId":"choice"})
    );
    assert_eq!(
        InteractionFailure::BrokerUnavailable.payload(),
        json!({"kind":"unavailable","stage":"inspect","message":"Approval service unavailable"})
    );

    // Observation.
    assert_eq!(
        ObservationFailure::InvalidField.rejection_reason(),
        Some(Reason::InvalidShape)
    );
    assert_eq!(ObservationFailure::NotFound.rejection_reason(), None);
    assert_eq!(
        serde_json::to_value(ObservationFailure::NotFound).expect("observation failure encodes"),
        json!({"kind":"notFound","stage":"inspect","message":"Provider Session was not found"})
    );
}

#[test]
fn a_result_too_large_for_its_response_is_not_the_concurrency_limit() {
    // Arrange: both keep the published "overloaded" payload kind.
    let native = NativeSessionFailure::refused(
        NativeSessionFailureKind::ResponseTooLarge,
        NativeSessionStage::Inspect,
    );
    let inventory =
        ProviderInventoryFailure::Unavailable(ProviderInventoryFailureKind::ResponseTooLarge);

    // Act & assert.
    assert_eq!(native.rejection_reason(), None);
    assert_eq!(wire_kind(&native), "overloaded");
    assert_eq!(inventory.rejection_reason(), None);
    assert_eq!(wire_kind(&inventory), "overloaded");
    assert_eq!(
        NativeSessionFailure::InvalidRequest(INVALID_NATIVE_PARAMETERS).rejection_reason(),
        Some(Reason::InvalidShape)
    );
    assert_eq!(
        ProviderInventoryFailure::InvalidRequest("Provider session page size must be 1..100")
            .rejection_reason(),
        Some(Reason::InvalidShape)
    );
}

#[test]
fn a_full_address_snapshot_cache_is_overload() {
    // Arrange.
    let full = JournalFailure::Unavailable(JournalUnavailableKind::Overloaded);
    let unknown_endpoint = JournalFailure::Unavailable(JournalUnavailableKind::EndpointNotFound);

    // Act & assert.
    assert_eq!(full.rejection_reason(), Some(Reason::Overloaded));
    assert_eq!(wire_kind(&full), "overloaded");
    assert_eq!(unknown_endpoint.rejection_reason(), None);
    assert_eq!(JournalFailure::SnapshotExpired.rejection_reason(), None);
    assert_eq!(
        JournalFailure::InvalidRequest.rejection_reason(),
        Some(Reason::InvalidShape)
    );
}
