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
    let mutation = output.batch.statements.first().unwrap();
    assert_eq!(mutation.expect, central_storage::RowExpectation::Exactly(1));
    assert_eq!(
        mutation.statement.args,
        vec![
            central_storage::SqlValue::Text("01890f2e-7b4c-7cc0-98c4-00000000001e".to_owned()),
            central_storage::SqlValue::Text("01890f2e-7b4c-7cc0-98c4-00000000000a".to_owned()),
            central_storage::SqlValue::Text("01890f2e-7b4c-7cc0-98c4-00000000000c".to_owned()),
            central_storage::SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned()),
            central_storage::SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned())
        ]
    );
    let removed = change_dependency(
        &context,
        &DependencyCommand::Remove {
            dependency_id: id(20),
        },
        DependencyReads {
            deliverables: &rows,
            dependencies: &edges,
        },
    )
    .unwrap();
    let mutation = removed.batch.statements.first().unwrap();
    assert_eq!(mutation.expect, central_storage::RowExpectation::Exactly(1));
    assert_eq!(
        mutation.statement.sql,
        "DELETE FROM deliverable_dependencies WHERE dependency_id=?"
    );
    assert_eq!(
        mutation.statement.args,
        vec![central_storage::SqlValue::Text(
            "01890f2e-7b4c-7cc0-98c4-000000000014".to_owned()
        )]
    );
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

// Independent matrix oracle ported from message-board's task-link property case.
fn transitive_closure(edges: &[[bool; 6]; 6]) -> [[bool; 6]; 6] {
    let mut reachable = *edges;
    for through in 0..6 {
        for from in 0..6 {
            for to in 0..6 {
                reachable[from][to] |= reachable[from][through] && reachable[through][to];
            }
        }
    }
    reachable
}
proptest::proptest! {
    #[test]
    fn ported_dependency_edit_sequences_preserve_acyclicity_and_atomic_edits(edits in proptest::collection::vec((0usize..6,0usize..6,proptest::prelude::any::<bool>()),0..100)) {
        let context=context();let rows:Vec<_>=(0..6).map(|value|deliverable(value+10)).collect();
        let mut edges=[[false;6];6];let mut dependencies=Vec::new();
        for (from,to,add) in edits {
            let dependency_id:workspace_model::DependencyId=id(100+u32::try_from(from*6+to).unwrap());
            let blocking=rows[from].deliverable_id.clone();let blocked=rows[to].deliverable_id.clone();
            if add {
                let reachable=transitive_closure(&edges);
                let result=change_dependency(&context,&DependencyCommand::Add {dependency_id:dependency_id.clone(),blocking:blocking.clone(),blocked:blocked.clone()},DependencyReads {deliverables:&rows,dependencies:&dependencies});
                if edges[from][to] {proptest::prop_assert!(matches!(result,Err(RefusalReason::DuplicateDependency {..})),"duplicate must be refused");}
                else if from == to || reachable[to][from] {proptest::prop_assert!(matches!(result,Err(RefusalReason::WouldCycle {..})),"cycle must be refused");}
                else {
                    let plan=result.unwrap();proptest::prop_assert_eq!(plan.batch.statements.len(),2);
                    proptest::prop_assert_eq!(plan.batch.statements.first().unwrap().statement.sql,"INSERT INTO deliverable_dependencies (dependency_id,blocking_deliverable_id,blocked_deliverable_id,created_at,updated_at) VALUES (?,?,?,?,?)");
                    let mutation=plan.batch.statements.first().unwrap();
                    proptest::prop_assert_eq!(mutation.expect,central_storage::RowExpectation::Exactly(1));
                    proptest::prop_assert_eq!(&mutation.statement.args,&vec![central_storage::SqlValue::Text(dependency_id.to_string()),central_storage::SqlValue::Text(blocking.to_string()),central_storage::SqlValue::Text(blocked.to_string()),central_storage::SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned()),central_storage::SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned())]);
                    edges[from][to]=true;dependencies.push(DeliverableDependency {dependency_id,blocking,blocked});
                }
            } else {
                let result=change_dependency(&context,&DependencyCommand::Remove {dependency_id:dependency_id.clone()},DependencyReads {deliverables:&rows,dependencies:&dependencies});
                if !edges[from][to] {proptest::prop_assert!(matches!(result,Err(RefusalReason::DependencyNotFound {..})),"missing dependency must refuse");continue;}
                let plan=result.unwrap();
                if edges[from][to] {let mutation=plan.batch.statements.first().unwrap();proptest::prop_assert_eq!(mutation.statement.sql,"DELETE FROM deliverable_dependencies WHERE dependency_id=?");proptest::prop_assert_eq!(mutation.expect,central_storage::RowExpectation::Exactly(1));proptest::prop_assert_eq!(&mutation.statement.args,&vec![central_storage::SqlValue::Text(dependency_id.to_string())]);}
                edges[from][to]=false;dependencies.retain(|row|row.dependency_id != dependency_id);
            }
            let reachable=transitive_closure(&edges);proptest::prop_assert!((0..6).all(|node|!reachable[node][node]));
        }
    }
}
