//! Automated archive outcomes return through the same native Session that reported.
use super::EffectDriver;
use crate::{ObjectHost, PlatformIo, fleet_archive_calls::FleetArchiveReport};
use board_sync_protocol::{ArchiveSeen, BoardConsumerAnswer, BoardConsumerProtocol, BoardPayload};
use durable_steps::Host as _;
use fleet_sync_protocol::{ConnectionId, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_archive_report(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        report: ArchiveSeen,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let Ok(identity) = self.shared_host.call_identity("archive_report") else {
            self.fail_connection(connection).await;
            return;
        };
        let outcome = self
            .shared_host
            .dispatch(FleetArchiveReport, report, identity)
            .await;
        let payload = match outcome.and_then(|outcome| {
            BoardPayload::encode(&outcome).map_err(|_| durable_steps::CallFailure::AnswerLost)
        }) {
            Ok(payload) => payload,
            Err(_) => {
                self.fail_connection(connection).await;
                return;
            }
        };
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let answer = OperationAnswer::Consumer(BoardConsumerAnswer::ArchiveSeen(payload));
        let bytes = match serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: answer,
        }) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.fail_connection(connection).await;
                return;
            }
        };
        if self
            .host
            .send_frame(connection, bytes.into())
            .await
            .is_err()
        {
            self.fail_connection(connection).await;
        }
    }
}
