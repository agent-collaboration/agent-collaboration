use super::*;

/// The canonical variants retain their exact ACP type and nested payload
/// shape at the single SDK decoding edge.
#[test]
fn audio_and_embedded_resources_keep_payload_and_capability_gate() {
    let blocks = acp_blocks_from_prompt_content(vec![
        PromptContent::audio("audio/wav".to_owned(), "aGVsbG8=".to_owned()).expect("audio"),
        PromptContent::embedded_text(
            "file:///note.txt".to_owned(),
            Some("text/plain".to_owned()),
            "hello".to_owned(),
        )
        .expect("text resource"),
        PromptContent::embedded_blob(
            "file:///data.bin".to_owned(),
            Some("application/octet-stream".to_owned()),
            "aGVsbG8=".to_owned(),
        )
        .expect("blob resource"),
    ]);
    assert_eq!(
        serde_json::to_value(&blocks).expect("ACP blocks"),
        serde_json::json!([
            {"type":"audio","mimeType":"audio/wav","data":"aGVsbG8="},
            {"type":"resource","resource":{"uri":"file:///note.txt",
                "mimeType":"text/plain","text":"hello"}},
            {"type":"resource","resource":{"uri":"file:///data.bin",
                "mimeType":"application/octet-stream","blob":"aGVsbG8="}}
        ])
    );
    assert!(matches!(
        ProviderPromptContent::new(blocks.clone(), &ProviderCapabilityReport::default()),
        Err(ExternalProviderRuntimeError::UnsupportedContent {
            content_type: "audio"
        })
    ));
    let mut capabilities = ProviderCapabilityReport {
        accepts_audio: true,
        ..ProviderCapabilityReport::default()
    };
    assert!(matches!(
        ProviderPromptContent::new(blocks.clone(), &capabilities),
        Err(ExternalProviderRuntimeError::UnsupportedContent {
            content_type: "embeddedResource"
        })
    ));
    capabilities.accepts_embedded_resource = true;
    assert!(ProviderPromptContent::new(blocks, &capabilities).is_ok());
}
