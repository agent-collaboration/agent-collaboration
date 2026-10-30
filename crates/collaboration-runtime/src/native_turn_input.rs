//! Native text, image and selected-skill decoding into the shared submission.
#[cfg(test)]
use super::json;
use super::{CommandContent, PathBuf, ThreadMethodError, Value};
const MAX_IMAGE_BYTES: usize = 768 * 1024;

pub(super) async fn parse_turn_input(
    params: &Value,
    session: &message_board::SessionRef,
) -> Result<session_event_model::ProviderSubmission, ThreadMethodError> {
    let input = params
        .get("input")
        .and_then(Value::as_array)
        .ok_or(ThreadMethodError::InvalidParams)?;
    let mut blocks = Vec::with_capacity(input.len());
    let mut selected = Vec::new();
    let mut encoded_bytes = 0usize;
    for entry in input {
        let block = match entry.get("type").and_then(Value::as_str) {
            Some("skill") => {
                let name = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(ThreadMethodError::InvalidSelection)?;
                let path = entry
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or(ThreadMethodError::InvalidSelection)?;
                let selection =
                    crate::native_skill_reference::decode_skill_reference(session, name, path)
                        .map_err(|_| ThreadMethodError::InvalidSelection)?;
                if !selected.contains(&selection) {
                    selected.push(selection);
                }
                continue;
            }
            Some("text") => entry
                .get("text")
                .and_then(Value::as_str)
                .ok_or(ThreadMethodError::InvalidParams)
                .and_then(|text| {
                    CommandContent::text(text.to_owned())
                        .map_err(|_| ThreadMethodError::InvalidParams)
                }),
            Some("localImage") => {
                let path = entry
                    .get("path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .ok_or(ThreadMethodError::InvalidParams)?;
                if !path.is_absolute() {
                    return Err(ThreadMethodError::InvalidParams);
                }
                let mime_type = image_mime_type(&path)?;
                let metadata = tokio::fs::metadata(&path)
                    .await
                    .map_err(|_| ThreadMethodError::ImageUnreadable)?;
                if metadata.len() > MAX_IMAGE_BYTES as u64 {
                    return Err(ThreadMethodError::ImageTooLarge);
                }
                let bytes = tokio::fs::read(&path)
                    .await
                    .map_err(|_| ThreadMethodError::ImageUnreadable)?;
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(ThreadMethodError::ImageTooLarge);
                }
                use base64::Engine as _;
                CommandContent::image(
                    mime_type.into(),
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                    None,
                )
                .map_err(|_| ThreadMethodError::InvalidParams)
            }
            Some("image") => {
                let url = entry
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or(ThreadMethodError::InvalidParams)?;
                let (media_type, encoded) = url
                    .split_once(",")
                    .ok_or(ThreadMethodError::InvalidParams)?;
                let mime_type = media_type
                    .strip_prefix("data:")
                    .and_then(|value| value.strip_suffix(";base64"))
                    .filter(|value| value.starts_with("image/"))
                    .ok_or(ThreadMethodError::ImageUnsupportedType)?;
                if encoded.len() > collaboration_protocol::MAX_CONTROL_FRAME_BYTES {
                    return Err(ThreadMethodError::ImageTooLarge);
                }
                use base64::Engine as _;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|_| ThreadMethodError::InvalidParams)?;
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(ThreadMethodError::ImageTooLarge);
                }
                CommandContent::image(mime_type.into(), encoded.to_owned(), None)
                    .map_err(|_| ThreadMethodError::InvalidParams)
            }
            _ => Err(ThreadMethodError::InvalidParams),
        }?;
        let block_bytes = match &block {
            CommandContent::Text { text } => text.as_str().len(),
            CommandContent::Image { data, .. } => data.as_str().len(),
            _ => 0,
        };
        encoded_bytes = encoded_bytes.saturating_add(block_bytes);
        if encoded_bytes > collaboration_protocol::MAX_CONTROL_FRAME_BYTES {
            return Err(ThreadMethodError::PromptTooLarge);
        }
        blocks.push(block);
    }
    Ok(session_event_model::ProviderSubmission {
        content: blocks,
        selected,
    })
}

fn image_mime_type(path: &std::path::Path) -> Result<&'static str, ThreadMethodError> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok("image/png"),
        Some("jpg" | "jpeg") => Ok("image/jpeg"),
        Some("webp") => Ok("image/webp"),
        Some("gif") => Ok("image/gif"),
        _ => Err(ThreadMethodError::ImageUnsupportedType),
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[tokio::test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions panic on a wrong result; Result propagates fixture and transport failures."
    )]
    async fn local_image_is_read_into_one_typed_image_block()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let session = test_session()?;
        let image_path = directory.path().join("sample.png");
        std::fs::write(&image_path, [0x89, b'P', b'N', b'G', 1, 2, 3])?;
        let input = json!({"input":[{"type":"text","text":"look"},
            {"type":"localImage","path":image_path}]});
        let blocks = parse_turn_input(&input, &session).await?;
        assert_eq!(
            blocks.content,
            vec![
                CommandContent::text("look".into())?,
                CommandContent::image("image/png".into(), "iVBORwECAw==".into(), None)?,
            ]
        );
        Ok(())
    }

    #[tokio::test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions panic on a wrong result; Result propagates fixture and transport failures."
    )]
    async fn image_input_reports_unreadable_and_oversized_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let session = test_session()?;
        let missing = directory.path().join("missing.png");
        let unreadable = parse_turn_input(
            &json!({"input":[{"type":"localImage","path":missing}]}),
            &session,
        )
        .await;
        assert!(matches!(
            unreadable,
            Err(ThreadMethodError::ImageUnreadable)
        ));

        let too_large = directory.path().join("large.png");
        std::fs::write(&too_large, vec![0u8; MAX_IMAGE_BYTES + 1])?;
        let oversized = parse_turn_input(
            &json!({"input":[{"type":"localImage","path":too_large}]}),
            &session,
        )
        .await;
        assert!(matches!(oversized, Err(ThreadMethodError::ImageTooLarge)));
        let inline = parse_turn_input(
            &json!({"input":[{"type":"image",
            "url":"data:image/png;base64,AQID"}]}),
            &session,
        )
        .await?;
        assert_eq!(
            inline.content,
            vec![CommandContent::image(
                "image/png".into(),
                "AQID".into(),
                None
            )?]
        );
        Ok(())
    }

    fn test_session() -> Result<message_board::SessionRef, serde_json::Error> {
        serde_json::from_value(
            json!({"endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},"sessionId":"target"}),
        )
    }

    #[tokio::test]
    async fn skill_references_validate_target_and_name_and_deduplicate_before_commands() {
        let session = test_session().expect("session");
        let input = json!({"input":[{"type":"text","text":"prefix $review args"},
            {"type":"skill","name":"review","path":"/__codex_router_skill__/claude-local/dGFyZ2V0/cmV2aWV3"},
            {"type":"skill","name":"review","path":"/__codex_router_skill__/claude-local/dGFyZ2V0/cmV2aWV3"}]});
        let submission = parse_turn_input(&input, &session)
            .await
            .expect("decoded selection");
        assert_eq!(
            submission.content,
            vec![CommandContent::text("prefix $review args".into()).expect("literal text")]
        );
        assert_eq!(submission.selected.len(), 1);
        assert_eq!(submission.selected[0].invocation_name.as_str(), "review");
        for path in [
            "/__codex_router_skill__/cursor-local/dGFyZ2V0/cmV2aWV3",
            "/__codex_router_skill__/claude-local/Zm9yZWlnbg/cmV2aWV3",
            "/__codex_router_skill__/claude-local/dGFyZ2V0/b3RoZXI",
        ] {
            let invalid = json!({"input":[{"type":"text","text":"$review"},{"type":"skill","name":"review","path":path}]});
            assert!(matches!(
                parse_turn_input(&invalid, &session).await,
                Err(ThreadMethodError::InvalidSelection)
            ));
        }
    }
}
