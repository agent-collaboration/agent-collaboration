//! Labelled authority is a projection input; this is not a central-copy acquisition proof.
use super::{ViewSnapshot, pending_projection::PendingWork, work_view};
use message_board::*;
use std::collections::BTreeMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn snapshot(
    source: CopySource,
    current_writer: CurrentWriter,
    as_of: FeedCursor,
    replicated_through: FeedCursor,
) -> ViewSnapshot {
    ViewSnapshot {
        project: ProjectId::generate(),
        records: BTreeMap::new(),
        creation: BTreeMap::new(),
        pending: PendingWork::default(),
        recent_trail: Vec::new(),
        counts: BTreeMap::new(),
        links: Vec::new(),
        as_of,
        current_writer,
        writer_observation: None,
        replicated_through,
        source,
    }
}

#[test]
#[allow(clippy::panic_in_result_fn)] // Assertions pin projection output; Result propagates fixture construction errors.
fn an_acknowledged_unheld_copy_keeps_its_position_when_a_grant_is_projected() -> TestResult {
    // Arrange C19's labelled generation and actual authority as explicit inputs.
    // Acquiring that generation from a real import belongs to A9/C33.
    let head = FeedCursor {
        epoch: EpochNumber::try_from(4)?,
        position: FeedPosition::try_from(17)?,
    };
    let mut read = snapshot(
        CopySource::ReadCopy {},
        CurrentWriter::Unheld {},
        head,
        head,
    );
    let unheld = work_view(&read, false)?;
    assert_eq!(unheld.current_writer, CurrentWriter::Unheld {});
    assert_eq!(unheld.source, CopySource::ReadCopy {});
    assert_eq!((unheld.as_of, unheld.replicated_through), (head, head));

    let granted = CurrentWriter::Held {
        epoch: EpochNumber::try_from(9)?,
    };
    read.current_writer = granted;
    let held = work_view(&read, false)?;
    assert_eq!(held.current_writer, granted);
    assert_eq!(held.source, CopySource::ReadCopy {});
    assert_eq!((held.as_of, held.replicated_through), (head, head));

    read.current_writer = CurrentWriter::Unknown {};
    let unreported = work_view(&read, false)?;
    assert_eq!(unreported.current_writer, CurrentWriter::Unknown {});
    assert_eq!(unreported.source, CopySource::ReadCopy {});
    assert_eq!(
        (unreported.as_of, unreported.replicated_through),
        (head, head)
    );
    Ok(())
}
