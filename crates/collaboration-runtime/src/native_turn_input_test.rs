use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture and transport failures."
)]
async fn local_image_is_read_into_one_typed_image_block() -> Result<(), Box<dyn std::error::Error>>
{
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
