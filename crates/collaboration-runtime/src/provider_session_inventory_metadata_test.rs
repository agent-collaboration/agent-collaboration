use super::*;

#[test]
fn title_updates_distinguish_missing_invalid_and_explicit_null() {
    // Arrange: a provider-supplied title is already known.
    let mut metadata = SessionInventoryMetadata::default();
    let mut observe = |text: &str| {
        metadata.observe_item(&SessionItem {
            item_id: uuid::Uuid::now_v7().to_string(),
            kind: SessionItemKind::SessionInfo,
            text: Some(text.into()),
        });
        metadata.name().map(str::to_owned)
    };
    assert_eq!(
        observe(r#"{"title":"Provider title"}"#).as_deref(),
        Some("Provider title")
    );
    // Act / Assert: omission preserves it, malformed data is not a title reset,
    // and explicit null alone clears it (ACP SessionInfoUpdate contract).
    for text in [
        r#"{"updatedAt":"2026-11-09T12:00:00Z"}"#,
        r#"{"title":42}"#,
        "invalid json",
    ] {
        assert_eq!(observe(text).as_deref(), Some("Provider title"));
    }
    assert_eq!(observe(r#"{"title":null}"#), None);
}

#[test]
fn non_user_content_cannot_become_the_resume_preview() {
    // Arrange: provider metadata and assistant output precede any user input.
    let mut metadata = SessionInventoryMetadata::default();
    for kind in [SessionItemKind::AgentMessage, SessionItemKind::SessionInfo] {
        // Act.
        metadata.observe_item(&SessionItem {
            item_id: uuid::Uuid::now_v7().to_string(),
            kind,
            text: Some(r#"{"title":"Reported title"}"#.into()),
        });
    }
    // Assert: the title remains separate from a still-empty user preview.
    assert_eq!(metadata.preview(), "");
    assert_eq!(metadata.name(), Some("Reported title"));
}
