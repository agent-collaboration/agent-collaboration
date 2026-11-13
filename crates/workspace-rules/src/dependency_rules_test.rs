use super::{DependencyReads, blocked_by, change_dependency};
use crate::fixtures::{context, deliverable, id};
use workspace_model::{DeliverableDependency, DependencyCommand, RefusalReason};
fn edge(value: u32, blocking: u32, blocked: u32) -> DeliverableDependency {
    DeliverableDependency {
        dependency_id: id(value),
        blocking: id(blocking),
        blocked: id(blocked),
    }
}
#[test]
fn r7_a_c_d_cycles_cross_project_duplicates_and_incoming_blockers() {
    let context = context();
    let mut rows = [deliverable(10), deliverable(11), deliverable(12)];
    let edges = [edge(20, 10, 11), edge(21, 11, 12)];
    let add = |a, b| DependencyCommand::Add {
        dependency_id: id(30),
        blocking: id(a),
        blocked: id(b),
    };
    assert!(
        matches!(change_dependency(&context,&add(12,10),DependencyReads {deliverables:&rows,dependencies:&edges}),Err(RefusalReason::WouldCycle {path}) if path == vec![id(12),id(10),id(11),id(12)])
    );
    assert!(matches!(
        change_dependency(
            &context,
            &add(10, 11),
            DependencyReads {
                deliverables: &rows,
                dependencies: &edges
            }
        ),
        Err(RefusalReason::DuplicateDependency { .. })
    ));
    assert_eq!(blocked_by(&id(12), &edges), vec![id(11)]);
    let output = change_dependency(
        &context,
        &add(10, 12),
        DependencyReads {
            deliverables: &rows,
            dependencies: &edges,
        },
    )
    .unwrap();
    assert!(output.batch.statements.iter().any(|row| {
        row.statement
            .sql
            .starts_with("INSERT INTO deliverable_dependencies")
    }));
    if let Some(row) = rows.last_mut() {
        row.project_id = id(99);
    }
    assert!(matches!(
        change_dependency(
            &context,
            &add(10, 12),
            DependencyReads {
                deliverables: &rows,
                dependencies: &edges
            }
        ),
        Err(RefusalReason::CrossProject { .. })
    ));
}
#[test]
fn ported_long_cycle_witness_keeps_the_real_path() {
    let rows: Vec<_> = (10..=310).map(deliverable).collect();
    let edges: Vec<_> = (10..310)
        .map(|value| edge(value + 1000, value, value + 1))
        .collect();
    let result = change_dependency(
        &context(),
        &DependencyCommand::Add {
            dependency_id: id(5000),
            blocking: id(310),
            blocked: id(10),
        },
        DependencyReads {
            deliverables: &rows,
            dependencies: &edges,
        },
    );
    assert!(
        matches!(result,Err(RefusalReason::WouldCycle {path}) if path.len() == 302 && path.first() == path.last())
    );
}
