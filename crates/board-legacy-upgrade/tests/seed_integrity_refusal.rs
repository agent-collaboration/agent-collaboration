//! The real publisher preserves an injected typed server refusal over HTTP.
#![allow(clippy::panic_in_result_fn)]
mod support;
use board_legacy_upgrade::{UpgradePublisher, UpgradeUploadError};
use collaboration_central_core::{
    AdminImportRefusal, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use fleet_sync_protocol::{RequestId, SeedAnswer, SeedId, TableName};
use support::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn seed_integrity_refusal_is_a_typed_incomplete_project_not_a_success() -> TestResult {
    let prepared = prepare("legacy-complete.sqlite").await?;
    let project = prepared
        .report
        .projects
        .first()
        .ok_or("prepared project")?
        .project
        .clone();
    let refusal = AdminProjectImportAnswer::Refused(AdminImportRefusal::SeedRefused(
        SeedAnswer::IntegrityViolation {
            table: TableName::new("posts")?,
            references: TableName::new("threads")?,
        },
    ));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let url = reqwest::Url::parse(&format!("http://{}", listener.local_addr()?))?;
    let offered = refusal.clone();
    let import_id = RequestId::new(uuid::Uuid::now_v7())?;
    let serving = tokio::spawn(async move {
        let mut phases = Vec::new();
        loop {
            let (mut stream, _) = listener.accept().await?;
            let (path, body) = read_request(&mut stream).await?;
            let (answer, done) = if path == "/admin/v1/import/fleet" {
                phases.push("fleet");
                (
                    serde_json::to_vec(&fleet_admission::FleetImportResult::AlreadyImported)?,
                    false,
                )
            } else {
                match serde_json::from_slice::<AdminProjectImportRequest>(&body)? {
                    AdminProjectImportRequest::Begin { .. } => {
                        phases.push("begin");
                        (
                            serde_json::to_vec(&AdminProjectImportAnswer::Ready {
                                import_id,
                                seed: SeedId { generation: 1 },
                            })?,
                            false,
                        )
                    }
                    AdminProjectImportRequest::Page { page, .. } => {
                        phases.push("page");
                        (
                            serde_json::to_vec(&AdminProjectImportAnswer::Staged {
                                import_id,
                                page,
                            })?,
                            false,
                        )
                    }
                    AdminProjectImportRequest::Complete { .. } => {
                        phases.push("complete");
                        (serde_json::to_vec(&offered)?, true)
                    }
                    request => {
                        return Err(std::io::Error::other(format!(
                            "unexpected import request: {request:?}"
                        ))
                        .into());
                    }
                }
            };
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",answer.len()).as_bytes()).await?;
            stream.write_all(&answer).await?;
            stream.shutdown().await?;
            if done {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(phases);
            }
        }
    });
    let observed=tokio::time::timeout(std::time::Duration::from_secs(30),async {
        let publisher=UpgradePublisher::new(url,reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(10)).build()?);
        let outcome=publisher.publish(&prepared).await;
        assert!(matches!(outcome,Err(UpgradeUploadError::ProjectIncomplete {project:actual,answer}) if actual==project && *answer==refusal),"typed seed integrity refusal was accepted or erased");
        Ok::<_,Box<dyn std::error::Error>>(())
    }).await;
    prepared.close().await;
    if !matches!(&observed, Ok(Ok(()))) {
        serving.abort();
    }
    observed??;
    let phases = serving
        .await?
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    assert_eq!(phases.first(), Some(&"fleet"));
    assert_eq!(phases.get(1), Some(&"begin"));
    assert!(
        phases[2..phases.len() - 1]
            .iter()
            .all(|phase| *phase == "page")
    );
    assert_eq!(phases.last(), Some(&"complete"));
    Ok(())
}
async fn read_request(
    stream: &mut tokio::net::TcpStream,
) -> Result<(String, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
    let mut received = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Err("incomplete HTTP request".into());
        }
        received.extend_from_slice(buffer.get(..count).ok_or("read exceeded fixture buffer")?);
        if let Some(end) = received.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break end + 4;
        }
        if received.len() > 65536 {
            return Err("oversized HTTP header".into());
        }
    };
    let header = std::str::from_utf8(
        received
            .get(..end)
            .ok_or("missing complete fixture header")?,
    )?;
    let path = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("request path")?
        .to_owned();
    let length = header
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.trim().parse::<usize>())
        })
        .ok_or("body length")??;
    if length > 2 * 1024 * 1024 {
        return Err("oversized fixture request".into());
    }
    while received.len() < end + length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Err("incomplete HTTP body".into());
        }
        received.extend_from_slice(buffer.get(..count).ok_or("read exceeded fixture buffer")?);
    }
    Ok((
        path,
        received
            .get(end..end + length)
            .ok_or("missing complete fixture body")?
            .to_vec(),
    ))
}
