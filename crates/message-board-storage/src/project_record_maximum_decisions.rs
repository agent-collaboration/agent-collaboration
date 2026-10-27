//! Decision size cases use real evidence producers and historical receipt metadata.
use super::{
    RECORD_BYTE_BUDGET, SCHEMA_BYTE_BUDGET, TestResult, dependency_cycle, ledger,
    measure_native_row, measure_native_row_with_effects, project_state, subject_assignments,
};
use message_board::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn decision_headers_and_full_or_abridged_evidence_fit_native_storage() -> TestResult {
    let state = project_state()?;
    // Closed witnesses with 255 and 256 nodes yield 256 full entries and a
    // 256-entry prefix respectively; assignments use the same boundary.
    let cases = [
        (
            "maximumFullCycle",
            dependency_cycle(&state, 255)?,
            256,
            false,
        ),
        (
            "maximumAbridgedCycle",
            dependency_cycle(&state, 256)?,
            257,
            true,
        ),
        (
            "maximumFullAssignments",
            subject_assignments(&state, 256)?,
            256,
            false,
        ),
        (
            "maximumAbridgedAssignments",
            subject_assignments(&state, 257)?,
            257,
            true,
        ),
    ];
    let mut measured = Vec::new();
    for (label, record, expected_count, expected_abridged) in cases {
        let Record::Request(row) = &record else {
            return Err("decision fixture".into());
        };
        let StoredDecision::Rejected { reasons } = &row.decision else {
            return Err("rejection fixture".into());
        };
        let (entries, count, abridged) = match reasons.as_slice() {
            [Refusal::Task(TaskRefusal::DependencyCycle { path })] => {
                (path.entries().len(), path.total_count(), path.is_abridged())
            }
            [Refusal::Trail(TrailRefusal::SubjectRequired { assignments })] => (
                assignments.entries().len(),
                assignments.total_count(),
                assignments.is_abridged(),
            ),
            _ => return Err("evidence fixture".into()),
        };
        assert_eq!(entries, 256, "{label} must reach the full entry bound");
        assert_eq!((count, abridged), (expected_count, expected_abridged));
        record.validate_bounds()?;
        measured.push(measure_native_row(label, &state, record).await?);
    }
    for (label, change, effects) in [
        ("acceptedChangedHeader", ChangeOutcome::Changed, 2),
        ("acceptedAlreadySoHeader", ChangeOutcome::AlreadySo, 0),
    ] {
        let record = ledger(&state, StoredDecision::accepted(Vec::new(), change))?;
        measured.push(measure_native_row_with_effects(label, &state, record, effects).await?);
    }
    for size in &measured {
        assert!(size.schema_statements > 0);
        assert!(size.schema_bytes <= SCHEMA_BYTE_BUDGET);
        assert!(
            size.conservative_numeric_row_bytes <= RECORD_BYTE_BUDGET,
            "{}: {} bytes",
            size.kind,
            size.conservative_numeric_row_bytes
        );
    }
    eprintln!(
        "BOARD_MAXIMUM_DECISION_SIZES {}",
        serde_json::to_string(&measured)?
    );
    Ok(())
}
