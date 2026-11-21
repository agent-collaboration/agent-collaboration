//! Generated crossings of the supervisor's reservation and write-custody decisions.

#[path = "provider_write_custody_property_test.rs"]
mod write_custody;

use super::{
    LiveOperation, LiveOperations, OperationId, ProviderBindingIdentity, ProviderOperationKind,
    SessionRef,
    provider_admission_policy::{AdmissionClass, AdmissionMode, AdmissionRequest, DispatchMark},
    provider_operation_index::Reservation,
    tests::binding,
};
use crate::provider_operation_custody::CustodyEnd;
use collaboration_protocol::SessionId;
use hegel::generators as gs;
use std::sync::Arc;

fn target(binding: &ProviderBindingIdentity, name: &str) -> SessionRef {
    SessionRef {
        endpoint: binding.endpoint.clone(),
        session_id: SessionId::try_from(name.to_owned()).expect("session ID"),
    }
}

fn independent_kind(target: Option<&SessionRef>) -> ProviderOperationKind {
    if target.is_some() {
        ProviderOperationKind::ConversationLoad
    } else {
        ProviderOperationKind::ConversationCreate
    }
}

fn reserve(
    live: &mut LiveOperations,
    binding: &ProviderBindingIdentity,
    request: AdmissionRequest<'_>,
) -> Result<Reservation, super::provider_admission_policy::AdmissionBlockers> {
    live.reserve(
        binding,
        request,
        &Arc::new(LiveOperation::reserving(
            binding,
            request.target.cloned(),
            request.kind,
            request.mode.mark,
        )),
        vec![],
        None,
    )
}

#[hegel::test(seed = Some(29115), test_cases = 100)]
fn generated_admission_rechecks_at_reservation(case: hegel::TestCase) {
    let binding = binding();
    let targets = [target(&binding, "first"), target(&binding, "second")];
    let first_id = OperationId::generate();
    let second_id = OperationId::generate();
    let first = AdmissionRequest {
        operation: &first_id,
        kind: ProviderOperationKind::ConversationCreate,
        target: None,
        mode: AdmissionMode::HANDOFF,
    };
    let second_target = case.draw(gs::booleans()).then_some(&targets[0]);
    let second = AdmissionRequest {
        operation: &second_id,
        kind: independent_kind(second_target),
        target: second_target,
        mode: AdmissionMode::HANDOFF,
    };

    // The competing preflight can see an empty index. Its reservation still
    // has to decide against the live index after the first request arrives.
    let mut live = LiveOperations::default();
    assert!(live.in_flight_blockers(&binding, second).is_empty());
    assert!(matches!(
        reserve(&mut live, &binding, first),
        Ok(Reservation::Acquired)
    ));
    let Err(blockers) = reserve(&mut live, &binding, second) else {
        panic!("I-b: a stale preflight let the second reservation overlap the first");
    };
    assert_eq!(blockers.in_flight, vec![first_id.clone()]);
    assert!(!live.operations.contains_key(&second_id));

    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(16)) {
        compare_admission(&case, &binding, &targets, &first_id, &second_id);
    }
}

fn compare_admission(
    case: &hegel::TestCase,
    binding: &ProviderBindingIdentity,
    targets: &[SessionRef; 2],
    first_id: &OperationId,
    second_id: &OperationId,
) {
    let choices = [None, Some(&targets[0]), Some(&targets[1])];
    let held_target = choices[usize::from(case.draw(gs::integers::<u8>().max_value(2)))];
    let asking_target = choices[usize::from(case.draw(gs::integers::<u8>().max_value(2)))];
    let same_generation = case.draw(gs::booleans());
    let reused = case.draw(gs::booleans());
    let holder_stage = case.draw(gs::integers::<u8>().max_value(5));
    let asking_port = case.draw(gs::integers::<u8>().max_value(4));
    let mut prior_binding = binding.clone();
    if !same_generation {
        prior_binding.generation.generation = 2.try_into().unwrap();
    }

    let holder = Arc::new(if holder_stage == 5 {
        LiveOperation::queued_prompt(&prior_binding, targets[0].clone())
    } else {
        LiveOperation::reserving(
            &prior_binding,
            held_target.cloned(),
            independent_kind(held_target),
            DispatchMark::BeforeHandoff,
        )
    });
    if (1..=4).contains(&holder_stage) {
        holder.admitted();
    }
    if (2..=4).contains(&holder_stage) {
        holder.dispatch_marked();
    }
    if (3..=4).contains(&holder_stage) {
        holder
            .completed(provider_session_machines::operation_record_machine::AttemptEnding::Applied);
    }
    if holder_stage == 4 {
        holder.finish_write(CustodyEnd::Written);
    }
    let mut live = LiveOperations::default();
    live.operations
        .insert(first_id.clone(), Arc::clone(&holder));
    let request = AdmissionRequest {
        operation: if reused { first_id } else { second_id },
        kind: match asking_port {
            0 => independent_kind(asking_target),
            1 => ProviderOperationKind::ConversationCancel,
            2 => ProviderOperationKind::ConversationClose,
            _ => ProviderOperationKind::ConversationPrompt,
        },
        target: asking_target,
        mode: match asking_port {
            3 => AdmissionMode::deferred(AdmissionClass::LinePlacement),
            4 => AdmissionMode::deferred(AdmissionClass::Independent),
            _ => AdmissionMode::HANDOFF,
        },
    };

    // The oracle describes the contract as a table, independently of the
    // admission predicate and the operation record machine.
    let overlaps = match (held_target, asking_target) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(holder), Some(requester)) => holder == requester,
    };
    let expected_block =
        holder_stage <= 3 && same_generation && matches!(asking_port, 0 | 4) && overlaps;
    match reserve(&mut live, binding, request) {
        Ok(Reservation::Occupied(prior)) => {
            assert!(reused, "a fresh ID was treated as a duplicate");
            assert!(
                Arc::ptr_eq(&prior, &holder),
                "I-g: ID reuse replaced its writer"
            );
        }
        Ok(Reservation::Acquired) => {
            assert!(!reused && !expected_block, "I-b: overlapping reservation")
        }
        Err(blockers) => {
            assert!(
                !reused && expected_block,
                "I-b: disjoint or exempt reservation refused"
            );
            assert_eq!(blockers.in_flight, vec![first_id.clone()]);
        }
    }
}
