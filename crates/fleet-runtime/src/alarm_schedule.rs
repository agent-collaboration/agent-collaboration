//! Chooses the next alarm mark from publication receipts and completed drives.
use fleet_contracts::{AlarmNotPublished, EventCompletion};
use fleet_runtime_ports::{Clock, SyncTime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlarmPublicationStatus {
    Published { deadline: Option<SyncTime> },
    Failed { at: SyncTime },
}

#[derive(Clone, Copy)]
pub enum AlarmDeadline<Deadline> {
    Until(Deadline),
    UntilPublication,
}

impl<Deadline> AlarmDeadline<Deadline> {
    fn from_mark(at: Option<SyncTime>, deadline: impl FnOnce(SyncTime) -> Deadline) -> Self {
        at.map_or(Self::UntilPublication, |mark| Self::Until(deadline(mark)))
    }
}

/// One active driver's retry state. A publication supersedes a pending retry.
pub struct AlarmSchedule<'clock, Deadline> {
    clock: &'clock dyn Clock,
    retry: Option<AlarmDeadline<Deadline>>,
}

impl<'clock, Deadline: Copy> AlarmSchedule<'clock, Deadline> {
    pub(super) fn new(clock: &'clock dyn Clock) -> Self {
        Self { clock, retry: None }
    }

    pub fn deadline(
        &self,
        publication: AlarmPublicationStatus,
        wall_deadline: impl FnOnce(SyncTime) -> Deadline,
    ) -> AlarmDeadline<Deadline> {
        self.retry.unwrap_or_else(|| {
            let mark = match publication {
                AlarmPublicationStatus::Published { deadline } => deadline,
                AlarmPublicationStatus::Failed { at } => retry_mark(at),
            };
            AlarmDeadline::from_mark(mark, wall_deadline)
        })
    }

    pub fn publication_changed(&mut self) {
        self.retry = None;
    }

    pub fn settle_drive(
        &mut self,
        result: &Result<EventCompletion, AlarmNotPublished>,
        wall_deadline: impl FnOnce(SyncTime) -> Deadline,
    ) {
        self.retry = match result {
            Ok(_) => None,
            Err(_) => Some(AlarmDeadline::from_mark(
                retry_mark(self.clock.now()),
                wall_deadline,
            )),
        };
    }
}

fn retry_mark(failed_at: SyncTime) -> Option<SyncTime> {
    let now = failed_at.unix_millis();
    central_storage::alarm_publication_time(Some(now), now)
        .ok()
        .flatten()
        .map(SyncTime::from_unix_millis)
}
