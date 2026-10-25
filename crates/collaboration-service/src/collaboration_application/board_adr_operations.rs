//! ADR lifecycle operations travel through the Fleet port.
use super::BoardOperations;
use super::board_fleet_operations::FleetHeader;
use super::board_paging::fingerprint;
use fleet_admission::FleetOperation;
use message_board::public_replies::WriteReply;
use message_board::*;
impl BoardOperations<'_> {
    pub async fn adr_create(&self, request: AdrCreateRequest) -> Result<WriteReply, BoardError> {
        let header = FleetHeader {
            request_id: request.request_id,
            protocol_version: request.protocol_version,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::AdrCreate {
                adr: request.adr_id,
                title: request.title,
                text: request.text,
                supersedes: request.supersedes,
                made_in: request.made_in,
                repository_files: request.repository_files,
            },
        )?)
        .await
    }
    pub async fn adr_transition(
        &self,
        request: AdrTransitionRequest,
    ) -> Result<WriteReply, BoardError> {
        let header = FleetHeader {
            request_id: request.request_id,
            protocol_version: request.protocol_version,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::AdrTransition {
                adr: request.adr_id,
                transition: request.transition,
                expected_revision: request.expected_revision,
            },
        )?)
        .await
    }
    pub async fn adr_show(&self, request: FleetRecordShowRequest) -> Result<Record, BoardError> {
        if request.record.kind != RecordKind::Adr {
            return Err(BoardError::invalid_field("record.kind", "must be adr"));
        }
        self.fleet_record(request.record).await
    }
    pub async fn adr_list(
        &self,
        request: FleetRecordListRequest,
    ) -> Result<RecordListResult, BoardError> {
        let filter = fingerprint(&request)?;
        let page = self
            .fleet_list_page(
                message_board_storage::FleetListFilter::Adrs {
                    workspace: &request.workspace_id,
                },
                request.page,
                "adr_list",
                filter,
            )
            .await?;
        Ok(RecordListResult {
            records: page.records,
            next_cursor: page.next_cursor,
        })
    }
}
