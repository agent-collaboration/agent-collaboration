//! Generated crossings of the supervisor's reservation and write-custody decisions.

#[path = "provider_write_custody_property_test.rs"]
mod write_custody;

use super::{
    LiveOperation, LiveOperations, OperationId, ProviderBindingIdentity, ProviderOperationKind,
    SessionRef,
    provider_admission_policy::{AdmissionClass, AdmissionMode, AdmissionRequest},
    provider_operation_index::Reservation,
    tests::binding,
};
use crate::provider_operation_custody::CustodyEnd;
use collaboration_protocol::SessionId;
use hegel::generators as gs;
use std::{collections::VecDeque, sync::Arc};

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
            request.mode,
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
    assert!(live.admission_blockers(&binding, second).is_empty());
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
        LiveOperation::queued_prompt(
            &prior_binding,
            targets[0].clone(),
            AdmissionMode::deferred(AdmissionClass::LinePlacement),
        )
    } else {
        LiveOperation::reserving(
            &prior_binding,
            held_target.cloned(),
            independent_kind(held_target),
            AdmissionMode::HANDOFF,
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
            .completed(crate::operation_machines::operation_record_machine::AttemptEnding::Applied);
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

struct ReservationEntry {
    name: &'static str,
    mode: AdmissionMode,
    rowless: bool,
    premark_admission: bool,
}

fn prompt_entries(foreground: uuid::Uuid) -> [ReservationEntry; 14] {
    use AdmissionClass::{ExactTurn, Independent, LinePlacement, RefuseWhileRunning};
    [
        ("direct prompt", Independent, false, true),
        ("idle Steer fallback", Independent, false, true),
        ("rowless idle Auto", Independent, true, true),
        ("rowless idle Compact", Independent, true, true),
        ("rowless idle evidenced Auto", Independent, true, true),
        ("Warm", Independent, false, true),
        ("direct delivery", Independent, false, true),
        ("Queue", LinePlacement, true, false),
        ("QueueOnRun", LinePlacement, true, false),
        ("running Auto", LinePlacement, false, false),
        ("loading Auto", LinePlacement, false, false),
        ("running Steer", ExactTurn, false, false),
        ("exact-turn Steer", ExactTurn, false, false),
        (
            "foreground refusal",
            RefuseWhileRunning { foreground },
            false,
            false,
        ),
    ]
    .map(
        |(name, class, rowless, premark_admission)| ReservationEntry {
            name,
            mode: AdmissionMode::deferred(class),
            rowless,
            premark_admission,
        },
    )
}

fn prompt_request<'a>(
    operation: &'a OperationId,
    target: &'a SessionRef,
    mode: AdmissionMode,
) -> AdmissionRequest<'a> {
    AdmissionRequest {
        operation,
        kind: ProviderOperationKind::ConversationPrompt,
        target: Some(target),
        mode,
    }
}

fn place_prompt(
    index: &mut LiveOperations,
    binding: &ProviderBindingIdentity,
    target: &SessionRef,
    operation: &OperationId,
    entry: &ReservationEntry,
) -> Arc<LiveOperation> {
    let request = prompt_request(operation, target, entry.mode);
    let live = Arc::new(if entry.rowless {
        LiveOperation::queued_prompt(binding, target.clone(), entry.mode)
    } else {
        LiveOperation::reserving(binding, Some(target.clone()), request.kind, entry.mode)
    });
    assert!(matches!(
        index.reserve(binding, request, &live, vec![], None),
        Ok(Reservation::Acquired)
    ));
    live
}

#[test]
fn the_reservation_class_table_keeps_admission_and_dispatch_holds_distinct() {
    let binding = binding();
    let session = target(&binding, "scope");
    let first = OperationId::generate();
    let second = OperationId::generate();
    let independent = AdmissionMode::deferred(AdmissionClass::Independent);
    let asks = prompt_request(&second, &session, independent);
    for entry in prompt_entries(uuid::Uuid::now_v7()) {
        let mut index = LiveOperations::default();
        let holder = place_prompt(&mut index, &binding, &session, &first, &entry);
        let before_mark = |index: &LiveOperations| {
            assert_eq!(
                index.admission_blockers(&binding, asks).contains(&first),
                entry.premark_admission,
                "{}",
                entry.name
            );
            assert_eq!(
                index.lane_mark_blocker(&binding, asks),
                None,
                "{}",
                entry.name
            );
        };
        before_mark(&index);
        if !entry.rowless {
            holder.admitted();
            before_mark(&index);
        }
        holder.dispatch_marked();
        assert_eq!(
            index.admission_blockers(&binding, asks),
            vec![first.clone()]
        );
        assert_eq!(index.lane_mark_blocker(&binding, asks), Some(first.clone()));
        holder.undispatched();
        before_mark(&index);
        holder.finish_write(CustodyEnd::NoWrite);
        assert!(index.admission_blockers(&binding, asks).is_empty());
        assert_eq!(index.lane_mark_blocker(&binding, asks), None);
    }
    for (kind, target, mode) in [
        (
            ProviderOperationKind::ConversationCreate,
            None,
            AdmissionMode::HANDOFF,
        ),
        (
            ProviderOperationKind::ConversationLoad,
            Some(&session),
            AdmissionMode::HANDOFF,
        ),
        (
            ProviderOperationKind::ConversationResume,
            Some(&session),
            AdmissionMode::HANDOFF,
        ),
        (
            ProviderOperationKind::ConversationClose,
            Some(&session),
            AdmissionMode::HANDOFF,
        ),
        (
            ProviderOperationKind::ConversationCancel,
            Some(&session),
            AdmissionMode::canceling(&OperationId::generate()),
        ),
    ] {
        let mut index = LiveOperations::default();
        assert!(matches!(
            reserve(
                &mut index,
                &binding,
                AdmissionRequest {
                    operation: &first,
                    kind,
                    target,
                    mode,
                }
            ),
            Ok(Reservation::Acquired)
        ));
        assert_eq!(
            index.admission_blockers(&binding, asks),
            vec![first.clone()],
            "{kind:?}"
        );
        assert_eq!(
            index.lane_mark_blocker(&binding, asks),
            Some(first.clone()),
            "{kind:?}"
        );
        index.operations[&first].admitted();
        assert_eq!(
            index.admission_blockers(&binding, asks),
            vec![first.clone()],
            "{kind:?}"
        );
        assert_eq!(
            index.lane_mark_blocker(&binding, asks),
            Some(first.clone()),
            "{kind:?}"
        );
    }
}

#[test]
fn claiming_an_actual_independent_start_reclassifies_the_exact_placement() {
    let binding = binding();
    let session = target(&binding, "same-session");
    let first = OperationId::generate();
    let second = OperationId::generate();
    let independent = AdmissionMode::deferred(AdmissionClass::Independent);
    for entry in prompt_entries(uuid::Uuid::now_v7()).into_iter().skip(9) {
        let mut index = LiveOperations::default();
        let live = place_prompt(&mut index, &binding, &session, &first, &entry);
        live.admitted();
        let claim = prompt_request(&first, &session, independent);
        let later = prompt_request(&second, &session, independent);
        assert_eq!(index.lane_mark_blocker(&binding, claim), None);
        assert!(index.admission_blockers(&binding, later).is_empty());
        assert_eq!(index.claim_independent_start(&binding, claim), None);
        assert_eq!(
            index.admission_blockers(&binding, later),
            vec![first.clone()]
        );
        assert_eq!(index.lane_mark_blocker(&binding, later), None);
    }
}

#[hegel::test(seed = Some(2912101), test_cases = 100)]
fn generated_same_target_reservations_obey_first_independent_hold(case: hegel::TestCase) {
    let binding = binding();
    let session = target(&binding, "shared");
    let ids = [OperationId::generate(), OperationId::generate()];
    let first = usize::from(case.draw(gs::booleans()));
    let (earlier, later) = (&ids[first], &ids[1 - first]);
    let entries = prompt_entries(uuid::Uuid::now_v7());
    let entry = &entries[case.draw(gs::integers::<usize>().max_value(entries.len() - 1))];
    let independent = AdmissionMode::deferred(AdmissionClass::Independent);
    let asks = prompt_request(later, &session, independent);
    let mut index = LiveOperations::default();
    let holder = place_prompt(&mut index, &binding, &session, earlier, entry);
    let stage = case.draw(gs::integers::<u8>().max_value(3));
    if stage >= 1 && !entry.rowless {
        holder.admitted();
    }
    if stage >= 2 {
        holder.dispatch_marked();
    }
    if stage == 3 {
        holder.finish_write(CustodyEnd::Written);
    }
    let blocked = stage == 2 || (stage < 2 && entry.premark_admission);
    match reserve(&mut index, &binding, asks) {
        Err(blockers) => {
            assert!(blocked, "{} {stage}", entry.name);
            assert_eq!(blockers.in_flight, vec![earlier.clone()]);
            assert!(!index.operations.contains_key(later));
        }
        Ok(Reservation::Acquired) => assert!(!blocked, "{} {stage}", entry.name),
        Ok(Reservation::Occupied(_)) => panic!("fresh request was treated as an ID reuse"),
    }
}

#[hegel::test(seed = Some(2912102), test_cases = 100)]
fn generated_owner_line_settles_after_external_holds_release(case: hegel::TestCase) {
    use crate::operation_machines::operation_record_machine::AttemptEnding;
    let binding = binding();
    let session = target(&binding, "owner-line");
    let ids = [OperationId::generate(), OperationId::generate()];
    let entries = prompt_entries(uuid::Uuid::now_v7());
    let independent = &entries[case.draw(gs::integers::<usize>().max_value(6))];
    let queued = &entries[7];
    let mut index = LiveOperations::default();
    let holds = [
        place_prompt(&mut index, &binding, &session, &ids[0], independent),
        place_prompt(&mut index, &binding, &session, &ids[1], queued),
    ];
    let handoff_id = OperationId::generate();
    let handoff = AdmissionRequest {
        operation: &handoff_id,
        kind: ProviderOperationKind::ConversationCreate,
        target: None,
        mode: AdmissionMode::HANDOFF,
    };
    let with_external_hold = case.draw(gs::booleans());
    if with_external_hold {
        assert!(matches!(
            reserve(&mut index, &binding, handoff),
            Ok(Reservation::Acquired)
        ));
    }
    let first = usize::from(case.draw(gs::booleans()));
    let mut owner_line = VecDeque::new();
    owner_line.push_back(first);
    if case.draw(gs::booleans()) {
        owner_line.push_back(1 - first);
    }
    let mut answers = [false; 2];
    let drain = |owner_line: &mut VecDeque<usize>, answers: &mut [bool; 2]| {
        while let Some(&head) = owner_line.front() {
            let request = prompt_request(&ids[head], &session, independent.mode);
            if let Some(blocker) = index.lane_mark_blocker(&binding, request) {
                assert_eq!(
                    blocker, handoff_id,
                    "a line head cannot wait on a prompt behind it"
                );
                assert!(!owner_line.iter().skip(1).any(|&tail| ids[tail] == blocker));
                return;
            }
            holds[head].admitted();
            holds[head].dispatch_marked();
            holds[head].completed(AttemptEnding::Applied);
            holds[head].finish_write(CustodyEnd::Written);
            answers[owner_line.pop_front().unwrap()] = true;
        }
    };
    drain(&mut owner_line, &mut answers);
    if with_external_hold {
        index.operations[&handoff_id].finish_write(CustodyEnd::NoWrite);
    }
    if !owner_line.contains(&(1 - first)) && !answers[1 - first] {
        owner_line.push_back(1 - first);
    }
    drain(&mut owner_line, &mut answers);
    assert_eq!(
        answers,
        [true, true],
        "all admitted inputs settle after the external hold"
    );
    assert!(owner_line.is_empty());
}
