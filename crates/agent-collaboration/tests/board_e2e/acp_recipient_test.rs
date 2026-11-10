use super::*;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn a_journal_write_signal_releases_the_pending_prompt_read() -> FixtureResult {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?
        .block_on(async {
    let directory = tempfile::tempdir()?;
    let recipient = AcpRecipient::arm(directory.path(), &identity(), "journal-signal-reader")?;
    let mut provider = tokio::process::Command::new(node_test_fixture::node_executable()?)
        .arg(node_test_fixture::fixture_program(
            "provider-loaded-route-agent.ts",
        ))
        .arg("journal")
        .arg(&recipient.journal)
        .arg(directory.path().join("acp-journal.sock"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = provider.stdin.take().ok_or("provider input absent")?;
    let mut output = BufReader::new(provider.stdout.take().ok_or("provider output absent")?);
    input.write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}})).as_bytes()).await?;
    let mut response = String::new();
    output.read_line(&mut response).await?;
    assert_eq!(
        serde_json::from_str::<Value>(&response)?.get("id"),
        Some(&json!(1))
    );
    let waiting = recipient.expect_text("literal journal signal");
    tokio::pin!(waiting);
    assert!(futures_util::poll!(&mut waiting).is_pending());

    let line = json!({"jsonrpc":"2.0","id":2,"method":"session/prompt","params": {
        "sessionId":recipient.target.session_id,
        "prompt":[{"type":"text","text":"literal journal signal"}]
    }});
    input.write_all(format!("{line}\n").as_bytes()).await?;

    response.clear();
    output.read_line(&mut response).await?;
    assert_eq!(
        serde_json::from_str::<Value>(&response)?.get("id"),
        Some(&json!(2))
    );
    assert_eq!(
        recipient.count()?,
        1,
        "the acknowledged prompt was not journaled"
    );
    assert_eq!(waiting.await?, "literal journal signal");
    drop(input);
    assert!(provider.wait().await?.success());
    Ok(())
        })
}

#[test]
fn an_already_written_prompt_needs_no_later_notification() -> FixtureResult {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?
        .block_on(async {
            let directory = tempfile::tempdir()?;
            let recipient =
                AcpRecipient::arm(directory.path(), &identity(), "journal-existing-reader")?;
            let line = json!({"method":"session/prompt","params": {
                "sessionId":recipient.target.session_id,
                "prompt":[{"type":"text","text":"existing literal prompt"}]
            }});
            std::fs::write(&recipient.journal, format!("{line}\n"))?;
            assert_eq!(
                recipient.expect_text("existing literal").await?,
                "existing literal prompt"
            );
            Ok(())
        })
}
