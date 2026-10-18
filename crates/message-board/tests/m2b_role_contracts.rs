use message_board::*;

#[test]
#[allow(clippy::panic_in_result_fn)] // Contract assertions; Result propagates fixture errors.
fn advisory_and_procedure_roles_do_not_obscure_a_single_implicit_trail_assignment()
-> Result<(), Box<dyn std::error::Error>> {
    let project = ProjectId::generate();
    let workstream = WorkstreamId::generate();
    let assignments = [
        Assignment::Lead(project.clone()),
        Assignment::Advisor(project),
        Assignment::Operator(workstream.clone()),
        Assignment::Reviewer(workstream),
    ];
    assert_eq!(
        work_rules::resolve_trail_subject(None, &assignments),
        Ok(TrailSubject::Project)
    );
    Ok(())
}

#[test]
fn missing_implicit_subject_lists_only_eligible_assignments() {
    let project = ProjectId::generate();
    let stream = WorkstreamId::generate();
    let advisory = [
        Assignment::Advisor(project.clone()),
        Assignment::Operator(stream.clone()),
        Assignment::Reviewer(stream.clone()),
    ];
    assert_eq!(
        work_rules::resolve_trail_subject(None, &advisory),
        Err(TrailRefusal::SubjectRequired {
            assignments: Vec::new()
        })
    );
    let lead = Assignment::Lead(project);
    let implementer = Assignment::Implementer(stream);
    let ambiguous = [vec![lead.clone(), implementer.clone()], advisory.to_vec()].concat();
    assert_eq!(
        work_rules::resolve_trail_subject(None, &ambiguous),
        Err(TrailRefusal::SubjectRequired {
            assignments: vec![lead, implementer]
        })
    );
}
