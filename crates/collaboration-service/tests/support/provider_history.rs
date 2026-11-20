use collaboration_runtime::{
    ProviderSessionEventHub,
    owner_inbox::{
        HubHistoryAvailability, HubPublication, HubPublicationBaseline, HubPublicationFact,
        HubPublicationStamp, HubSessionInbox,
    },
};
use message_board::SessionRef;
use session_event_model::{SessionEvent, SessionSettings, SessionState, SkillView};
use std::{num::NonZeroUsize, sync::Arc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(crate) struct ResumePublicationFixture {
    inbox: HubSessionInbox,
    incarnation: Uuid,
    stop: CancellationToken,
    worker: JoinHandle<()>,
}

impl ResumePublicationFixture {
    pub(crate) async fn start(
        hub: &Arc<ProviderSessionEventHub>,
        target: &SessionRef,
    ) -> TestResult<Self> {
        let incarnation = Uuid::now_v7();
        let (inbox, task) = hub.owner_inbox(target.clone(), NonZeroUsize::MIN);
        inbox.install_incarnation(None, incarnation)?;
        let stop = CancellationToken::new();
        let worker = tokio::spawn(
            task.ok_or("fixture needs the first owner inbox")?
                .run(stop.clone()),
        );
        let fixture = Self {
            inbox,
            incarnation,
            stop,
            worker,
        };
        fixture
            .inbox
            .try_offer(HubPublication::Rebase {
                stamp: fixture.stamp(1),
                baseline: Box::new(HubPublicationBaseline {
                    state: SessionState::Unloaded,
                    turn_running: false,
                    capabilities: None,
                    settings: None,
                    skills: SkillView::Unknown,
                    history: HubHistoryAvailability::Available,
                }),
            })?
            .await??;
        Ok(fixture)
    }

    pub(crate) async fn resume(&self, settings: Option<SessionSettings>) -> TestResult {
        let mut facts = vec![HubPublicationFact::HistoryUnavailable];
        if let Some(settings) = settings {
            facts.push(HubPublicationFact::Event(SessionEvent::SettingsChanged {
                settings,
            }));
        }
        facts.push(HubPublicationFact::Event(SessionEvent::StateChanged {
            state: SessionState::Idle,
        }));
        self.inbox
            .try_offer(HubPublication::Commit {
                stamp: self.stamp(2),
                facts,
                observed_at_seconds: 0,
            })?
            .await??;
        Ok(())
    }

    pub(crate) async fn stop(self) -> TestResult {
        self.stop.cancel();
        self.worker.await?;
        Ok(())
    }

    fn stamp(&self, sequence: u64) -> HubPublicationStamp {
        HubPublicationStamp {
            incarnation: self.incarnation,
            sequence,
        }
    }
}
