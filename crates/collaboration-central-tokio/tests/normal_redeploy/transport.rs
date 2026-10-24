//! Public socket correlation and literal snapshots shared by the normal-state rows.
use super::*;

pub(super) async fn next(
    socket: &mut WebSocket,
) -> ConformanceResult<ServerFrame<BoardConsumerProtocol>> {
    let message = tokio::time::timeout(Duration::from_secs(8), socket.next())
        .await?
        .ok_or("normal-state socket closed before its data frame")??;
    match message {
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        other => Err(format!("normal-state socket expected data, got {other:?}").into()),
    }
}
pub(super) async fn reconnect(
    base: &str,
    credential: &str,
    session_id: SessionId,
) -> ConformanceResult<WebSocket> {
    let mut socket = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .get(format!("{base}/v1/session"))
        .bearer_auth(credential)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(credential)?)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id,
                schema_support: SchemaSupport { max_version: 10 },
            })?
            .into(),
        ))
        .await?;
    let proof = async {
        loop {
            match next(&mut socket).await? {
                ServerFrame::Notice { .. } => {}
                ServerFrame::Welcome {
                    session_id: actual, ..
                } if actual == session_id => {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                _ => {
                    return Err(
                        "normal-state reconnect did not return its exact session Welcome".into(),
                    );
                }
            }
        }
    }
    .await;
    if let Err(error) = proof {
        let _ = socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        return Err(error);
    }
    Ok(socket)
}
pub(super) async fn acked(
    socket: &mut WebSocket,
    piece: &Piece<BoardAnnotation>,
    through: ReplicationPosition,
) -> ConformanceResult<()> {
    let answer = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::Piece(piece.clone())),
    )
    .await?;
    let diagnostic = serde_json::to_string(&answer)?;
    if !matches!(answer,OperationAnswer::Replication(ReplicationAnswer::Applied {through:actual}|ReplicationAnswer::AlreadyApplied {through:actual}) if actual==through)
    {
        return Err(format!(
            "normal-state Piece lost its exact acknowledgement {through:?}: {diagnostic}"
        )
        .into());
    }
    Ok(())
}
pub(super) async fn snapshot(
    socket: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<SnapshotPage> {
    match creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        }),
    )
    .await?
    {
        OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page)))
            if page.database == database
                && page.table == "records"
                && page.schema.version == 1
                && page.next.is_none() =>
        {
            Ok(*page)
        }
        other => Err(format!(
            "normal-state snapshot is not its complete records page: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}
pub(super) fn expected(rows: &[(i64, &str)]) -> ConformanceResult<Vec<Vec<SqlValue>>> {
    rows.iter()
        .map(|(id, label)| {
            Ok(vec![
                SqlValue::Text(registry_setup::normal_record_key(*id)?.into()),
                SqlValue::Integer(*id),
                SqlValue::Text((*label).into()),
            ])
        })
        .collect()
}
pub(super) async fn exact(
    socket: &mut WebSocket,
    database: DatabaseId,
    position: ReplicationPosition,
    rows: &[(i64, &str)],
) -> ConformanceResult<()> {
    let page = snapshot(socket, database).await?;
    if page.position != position || page.rows != expected(rows)? {
        return Err(
            format!("normal-state changed literal complete rows or position: {page:?}").into(),
        );
    }
    Ok(())
}
