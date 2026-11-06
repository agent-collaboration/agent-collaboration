use super::check_writer;
use message_board::{CopySource, CurrentWriter, EpochNumber, FeedCursor, ProjectId, WorkView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn inherited_view(
    project: ProjectId,
    position: u64,
) -> Result<WorkView, Box<dyn std::error::Error>> {
    let head = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: position.try_into()?,
    };
    Ok(WorkView {
        project,
        as_of: head,
        replicated_through: head,
        workstreams: vec![],
        ready_queue: vec![],
        blocked: vec![],
        open_in_closed_workstreams: vec![],
        current_writer: CurrentWriter::Held {
            epoch: EpochNumber::try_from(2)?,
        },
        source: CopySource::Writer {},
        writer_observation: None,
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real e2 writer may retain an unchanged e1 seed cursor."
)]
fn canary_accepts_an_e2_writer_with_its_exact_inherited_e1_history() -> TestResult {
    // Arrange an e2 authority whose unchanged application history is still e1.
    let view = inherited_view(ProjectId::generate(), 12)?;
    // Act / Assert: current authority, rather than a previously emitted application write, admits the canary.
    assert!(check_writer(&view, &view.project, view.as_of).is_ok());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A matching application cursor never substitutes for e2 authority."
)]
fn canary_rejects_foreign_stale_read_copy_and_wrong_epoch_views() -> TestResult {
    // Arrange a fixed sealed application head and its one allowed e2 writer.
    let view = inherited_view(ProjectId::generate(), 12)?;
    let project = view.project.clone();
    let source_head = view.as_of;
    let mut cases = Vec::new();
    let mut foreign = view.clone();
    foreign.project = ProjectId::generate();
    cases.push(foreign);
    let mut stale = view.clone();
    stale.as_of.position = 11_u64.try_into()?;
    cases.push(stale);
    let mut false_epoch = view.clone();
    false_epoch.as_of.epoch = EpochNumber::try_from(2)?;
    cases.push(false_epoch);
    let mut future = view.clone();
    future.as_of.epoch = EpochNumber::try_from(3)?;
    cases.push(future);
    let mut copy = view.clone();
    copy.source = CopySource::ReadCopy {};
    cases.push(copy);
    for writer in [
        CurrentWriter::Unheld {},
        CurrentWriter::Unknown {},
        CurrentWriter::NotInCentral {},
        CurrentWriter::Held {
            epoch: EpochNumber::INITIAL,
        },
        CurrentWriter::Held {
            epoch: EpochNumber::try_from(3)?,
        },
    ] {
        let mut wrong_writer = view.clone();
        wrong_writer.current_writer = writer;
        cases.push(wrong_writer);
    }
    // Act / Assert: every foreign, stale or unheld view refuses before a public write.
    for rejected in cases {
        assert!(check_writer(&rejected, &project, source_head).is_err());
    }
    // A later real e2 application head remains allowed after normal recovery or the canary.
    let mut recovered = view;
    recovered.as_of = FeedCursor {
        epoch: EpochNumber::try_from(2)?,
        position: 14_u64.try_into()?,
    };
    assert!(check_writer(&recovered, &project, source_head).is_ok());
    Ok(())
}
