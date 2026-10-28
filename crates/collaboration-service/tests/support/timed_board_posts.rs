//! Real native records with a controlled UTC clock, as in aggregate_cursor_order.
use super::board_fixture::{BoardFixture, TestError};
use message_board::*;

pub struct TimedPostInput<'input> {
    pub time: &'input str,
    pub thread: Option<ThreadId>,
}
pub async fn append_post_at(
    fixture: &BoardFixture,
    project: &ProjectId,
    input: TimedPostInput<'_>,
) -> Result<PostId, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let base = read
        .record(&project.clone().into())
        .await?
        .ok_or("project record")?;
    let mut envelope = base.envelope().clone();
    read.finish().await?;
    let post_id = PostId::generate();
    let actor = Identity::Human {
        human_id: post_id.as_str().to_owned().try_into()?,
    };
    envelope.id = post_id.clone().into();
    envelope.actor = RecordedActor::declared(actor.clone());
    envelope.request = None;
    envelope.time = RecordTime::Router(input.time.parse()?);
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("V1/V2 proof".to_owned())?,
    };
    envelope.position = RecordPlacement::Project(ProjectFeedEntry {
        cursor: FeedCursor {
            epoch: EpochNumber::try_from(1)?,
            position: FeedPosition::ZERO,
        },
        origin: origin.clone(),
    });
    let mut write = store.begin_admission(origin).await?;
    let placement = match input.thread {
        Some(thread_id) => Placement::Thread { thread_id },
        None => {
            let thread_id = ThreadId::from(post_id.clone());
            write
                .append_record(Record::Thread(Thread {
                    envelope: envelope.clone(),
                    thread_id,
                    about: ThreadSubject::Project {
                        line: "Timed fixture discussion".to_owned().try_into()?,
                    },
                    imported: false,
                    state: ThreadState::Open,
                    orchestrator: None,
                    implementer: None,
                }))
                .await?;
            Placement::Project {
                project_id: project.clone(),
            }
        }
    };
    write
        .append_record(Record::Post(Post {
            envelope: envelope.clone(),
            post_id: post_id.clone(),
            board_id: fixture.board.clone(),
            project_id: project.clone(),
            placement,
            actor,
            acting_for: None,
            text: input.time.to_owned().try_into()?,
            references: PostReferences::try_from(Vec::new())?,
            position: envelope.position,
            posted_as_role: None,
        }))
        .await?;
    write.commit().await?;
    Ok(post_id)
}
