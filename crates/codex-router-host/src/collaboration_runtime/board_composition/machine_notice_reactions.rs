//! Machine hints reconcile authoritative owners before their separate progress is saved.
use super::project_writers::ProjectWriters;
use board_fleet_sync::MachineNoticeProgressStore;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardNotice, BoardReadCursor,
    BoardReadPage, BoardReadScope, NoticeRead, PositionedBoardNotice,
};
use collaboration_service::{FleetClient, MailDeliveryRuntime, ProjectAdmission};
use fleet_sync_protocol::{NoticePosition, OperationAnswer, OperationRequest};
use fleet_sync_tokio_runner::SessionRequests;
use std::{
    io,
    path::Path,
    sync::{Arc, Weak},
};

pub(super) struct MachineNoticeReactions {
    progress: MachineNoticeProgressStore,
    handled_through: NoticePosition,
}

impl MachineNoticeReactions {
    pub async fn open(directory: &Path) -> io::Result<Self> {
        let progress = MachineNoticeProgressStore::open(directory)
            .await
            .map_err(io::Error::other)?;
        let handled_through = progress.load().await.map_err(io::Error::other)?;
        Ok(Self {
            progress,
            handled_through,
        })
    }

    pub async fn handle_pushed_notice(
        &mut self,
        notice: PositionedBoardNotice,
        projects: &Arc<ProjectWriters>,
        admission: &Weak<ProjectAdmission>,
        fleet: &Weak<FleetClient>,
        mail: &Weak<MailDeliveryRuntime>,
        requests: &SessionRequests<BoardConsumerProtocol>,
    ) -> io::Result<()> {
        if notice.position <= self.handled_through {
            return Ok(());
        }
        if self.handled_through.next() != Some(notice.position) {
            self.catch_up_notices(projects, admission, fleet, mail, requests)
                .await?;
        }
        if notice.position <= self.handled_through {
            return Ok(());
        }
        if self.handled_through.next() != Some(notice.position) {
            return Err(invalid_notice("machine notice gap remains after catch-up"));
        }
        reconcile_notice(&notice.kind, projects, admission, fleet, mail).await?;
        self.save(notice.position).await
    }

    /// Welcome also reads after the saved machine cursor, so a restored central
    /// head below it is observed even when every new push looks like a duplicate.
    pub async fn catch_up_notices(
        &mut self,
        projects: &Arc<ProjectWriters>,
        admission: &Weak<ProjectAdmission>,
        fleet: &Weak<FleetClient>,
        mail: &Weak<MailDeliveryRuntime>,
        requests: &SessionRequests<BoardConsumerProtocol>,
    ) -> io::Result<()> {
        let mut after = Some(self.handled_through);
        loop {
            match read_notices(requests, after).await? {
                NoticeRead::Page { notices, .. } => {
                    if notices.is_empty() {
                        return Ok(());
                    }
                    let before = self.handled_through;
                    let mut retained_start = after.is_none();
                    for notice in notices {
                        if notice.position <= self.handled_through {
                            continue;
                        }
                        if !retained_start && self.handled_through.next() != Some(notice.position) {
                            return Err(invalid_notice("machine notice page is not contiguous"));
                        }
                        reconcile_notice(&notice.kind, projects, admission, fleet, mail).await?;
                        self.save(notice.position).await?;
                        retained_start = false;
                    }
                    if self.handled_through == before {
                        return Err(invalid_notice("machine notice page did not advance"));
                    }
                    after = Some(self.handled_through);
                }
                NoticeRead::CursorExpired { floor } => {
                    if after.is_none() || floor <= self.handled_through {
                        return Err(invalid_notice("expired notice floor did not advance"));
                    }
                    recover_owners(projects, admission, fleet, mail).await?;
                    self.save(floor).await?;
                    after = Some(floor);
                }
                NoticeRead::CursorInvalid => {
                    if after.is_none() {
                        return Err(invalid_notice("notice read without a cursor was invalid"));
                    }
                    recover_owners(projects, admission, fleet, mail).await?;
                    self.save(NoticePosition::new(0)).await?;
                    after = None;
                }
            }
        }
    }

    async fn save(&mut self, position: NoticePosition) -> io::Result<()> {
        self.progress
            .save(position)
            .await
            .map_err(io::Error::other)?;
        self.handled_through = position;
        Ok(())
    }
}

async fn read_notices(
    requests: &SessionRequests<BoardConsumerProtocol>,
    after: Option<NoticePosition>,
) -> io::Result<NoticeRead> {
    let answer = requests
        .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: after.map(BoardReadCursor::Notices),
        }))
        .await
        .map_err(io::Error::other)?;
    match answer {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(page))) => {
            Ok(page)
        }
        _ => Err(invalid_notice(
            "machine notice read returned another answer",
        )),
    }
}

async fn recover_owners(
    projects: &Arc<ProjectWriters>,
    admission: &Weak<ProjectAdmission>,
    fleet: &Weak<FleetClient>,
    mail: &Weak<MailDeliveryRuntime>,
) -> io::Result<()> {
    if let Some(mail) = mail.upgrade() {
        mail.prompt_receive_all().await.map_err(io::Error::other)?;
    }
    projects.prompt_all_renewals().await?;
    super::archive_observation::synchronize_and_observe(projects, admission, fleet).await
}

// Hints never apply their claimed epoch or release directly. Rehandling may
// enqueue another ordinary renewal or read; authoritative replies decide state.
async fn reconcile_notice(
    notice: &BoardNotice,
    projects: &Arc<ProjectWriters>,
    admission: &Weak<ProjectAdmission>,
    fleet: &Weak<FleetClient>,
    mail: &Weak<MailDeliveryRuntime>,
) -> io::Result<()> {
    match notice {
        BoardNotice::LeaseGranted { project, epoch } => {
            projects.receive_grant(project, *epoch).await
        }
        BoardNotice::LeaseSuperseded { project, .. } => {
            projects.prompt_superseded_writer(project).await
        }
        BoardNotice::FleetFeedAdvanced { .. } | BoardNotice::BoardArchiving { .. } => {
            super::archive_observation::synchronize_and_observe(projects, admission, fleet).await
        }
        BoardNotice::MailWaiting { stream } => {
            let mail = mail
                .upgrade()
                .ok_or_else(|| invalid_notice("mail runtime has closed"))?;
            mail.prompt_receive(*stream).await.map_err(io::Error::other)
        }
    }
}

fn invalid_notice(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
