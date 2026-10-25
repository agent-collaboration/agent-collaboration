//! Shared normal-session fixture for native and Worker board scenarios.
use crate::{
    ConformanceResult,
    scenario_assertions::{check, equal},
};
use fleet_admission::{FleetRequest, FleetRequestResult};
use message_board::{Record, RequestDecision};

pub(crate) struct NativeBoardSession(pub(crate) crate::AdmittedPeerSocket);
impl NativeBoardSession {
    pub(crate) async fn request(
        &mut self,
        request: board_sync_protocol::BoardConsumerRequest,
    ) -> ConformanceResult<board_sync_protocol::BoardConsumerAnswer> {
        use board_sync_protocol::BoardConsumerProtocol;
        use fleet_sync_protocol::{
            ClientFrame, FrameId, OperationAnswer, OperationRequest, ServerFrame,
        };
        use futures_util::{SinkExt, StreamExt};
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        self.0
            .socket
            .send(reqwest_websocket::Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                    frame,
                    body: OperationRequest::Consumer(request),
                })?
                .into(),
            ))
            .await?;
        loop {
            let next =
                tokio::time::timeout(std::time::Duration::from_secs(10), self.0.socket.next())
                    .await?
                    .ok_or("board session closed")??;
            let bytes = match next {
                reqwest_websocket::Message::Binary(bytes) => bytes,
                reqwest_websocket::Message::Text(text) => text.into_bytes().into(),
                _ => continue,
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Answer {
                    frame: actual,
                    body: OperationAnswer::Consumer(answer),
                } if actual == frame => return Ok(answer),
                ServerFrame::Notice { .. } => {}
                _ => return Err("unexpected native board frame or correlation".into()),
            }
        }
    }

    pub(crate) async fn write(
        &mut self,
        request: &FleetRequest,
    ) -> ConformanceResult<RequestDecision> {
        use board_sync_protocol::{
            BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, FleetRequestEnvelope,
        };
        let BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        } = self
            .request(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
                request_id: request.request_id.clone(),
                operation: BoardPayload::encode(request)?,
            }))
            .await?
        else {
            return Err("native board request did not return a Fleet decision".into());
        };
        equal(request_id, request.request_id.clone())?;
        let FleetRequestResult::Decided(decision) = decision.decode()? else {
            return Err("native board request conflicted".into());
        };
        Ok(decision)
    }

    pub(crate) async fn snapshot(&mut self) -> ConformanceResult<(Vec<Record>, u64)> {
        use board_sync_protocol::{
            BoardConsumerAnswer, BoardConsumerRequest, BoardReadCursor, BoardReadPage,
            BoardReadScope,
        };
        let mut cursor = None;
        let mut cut = None;
        let mut records = Vec::new();
        loop {
            let BoardConsumerAnswer::Read(BoardReadPage::FleetSnapshot {
                records: page,
                next,
                snapshot_at,
                ..
            }) = self
                .request(BoardConsumerRequest::Read {
                    scope: BoardReadScope::FleetSnapshot,
                    cursor: cursor.clone(),
                })
                .await?
            else {
                return Err("expected native board fixture snapshot page".into());
            };
            if let Some(first) = cut {
                equal(snapshot_at, first)?;
            } else {
                cut = Some(snapshot_at);
            }
            records.extend(
                page.into_iter()
                    .map(|record| record.decode::<Record>())
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let next = next.map(BoardReadCursor::FleetSnapshot);
            if next.is_none() {
                return Ok((records, snapshot_at.get()));
            }
            check(next != cursor, "native board snapshot cursor advances")?;
            cursor = next;
        }
    }

    pub(crate) async fn close(self) -> ConformanceResult<()> {
        self.0
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
}
