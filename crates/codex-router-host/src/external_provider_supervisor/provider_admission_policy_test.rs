use super::*;
use collaboration_protocol::SessionId;

fn session(name: &str) -> SessionRef {
    SessionRef {
        endpoint: super::super::tests::binding().endpoint,
        session_id: SessionId::try_from(name.to_owned()).expect("session ID"),
    }
}

#[test]
fn the_live_overlap_matrix_is_directional() {
    let earlier = OperationId::generate();
    let operation = OperationId::generate();
    let first = session("first");
    let second = session("second");
    let cases = [
        (None, None, true),
        (None, Some(&first), true),
        (Some(&first), Some(&first), true),
        (Some(&first), Some(&second), false),
        (Some(&first), None, false),
    ];
    for (holder, target, expected) in cases {
        let request = AdmissionRequest {
            operation: &operation,
            kind: if target.is_some() {
                ProviderOperationKind::ConversationLoad
            } else {
                ProviderOperationKind::ConversationCreate
            },
            target,
            mode: AdmissionMode::HANDOFF,
        };
        assert_eq!(
            request.blocked_by(
                &earlier,
                EarlierOperation::InFlight { target: holder },
                None
            ),
            expected,
            "holder {holder:?}, requester {target:?}"
        );
    }
}

#[test]
fn close_can_interrupt_every_live_holder() {
    let target = session("closing");
    let operation = OperationId::generate();
    let earlier = OperationId::generate();
    let request = AdmissionRequest {
        operation: &operation,
        kind: ProviderOperationKind::ConversationClose,
        target: Some(&target),
        mode: AdmissionMode::HANDOFF,
    };
    for holder in [None, Some(&target)] {
        assert!(!request.blocked_by(
            &earlier,
            EarlierOperation::InFlight { target: holder },
            None
        ));
    }
}

#[test]
fn refuse_busy_leaves_only_the_exact_running_turn_to_the_owner() {
    let target = session("running");
    let requester = OperationId::generate();
    let foreground = OperationId::generate();
    let other = OperationId::generate();
    let request = AdmissionRequest {
        operation: &requester,
        kind: ProviderOperationKind::ConversationPrompt,
        target: Some(&target),
        mode: AdmissionMode::deferred(AdmissionClass::RefuseWhileRunning {
            foreground: uuid::Uuid::parse_str(foreground.as_str()).unwrap(),
        }),
    };
    assert!(!request.blocked_by(
        &foreground,
        EarlierOperation::InFlight {
            target: Some(&target)
        },
        None
    ));
    assert!(request.blocked_by(
        &other,
        EarlierOperation::InFlight {
            target: Some(&target)
        },
        None
    ));
    assert!(request.blocked_by(&other, EarlierOperation::InFlight { target: None }, None));
    let independent = AdmissionRequest {
        mode: AdmissionMode::deferred(AdmissionClass::Independent),
        ..request
    };
    assert!(independent.blocked_by(
        &foreground,
        EarlierOperation::InFlight {
            target: Some(&target)
        },
        None
    ));
}

#[test]
fn a_lane_mark_excludes_only_the_cancel_that_names_it() {
    let operation = OperationId::generate();
    let target = session("queued");
    let request = AdmissionRequest {
        operation: &operation,
        kind: ProviderOperationKind::ConversationPrompt,
        target: Some(&target),
        mode: AdmissionMode::deferred(AdmissionClass::Independent),
    };
    let exact = uuid::Uuid::parse_str(operation.as_str()).unwrap();
    assert!(
        request.is_own_cancel_holder(Some(ProviderOperationKind::ConversationCancel), Some(exact))
    );
    assert!(!request.is_own_cancel_holder(
        Some(ProviderOperationKind::ConversationCancel),
        Some(uuid::Uuid::now_v7())
    ));
    assert!(
        !request.is_own_cancel_holder(Some(ProviderOperationKind::ConversationClose), Some(exact))
    );
    assert!(request.blocked_by(
        &OperationId::generate(),
        EarlierOperation::InFlight {
            target: Some(&target)
        },
        None
    ));
}

#[test]
fn close_only_excludes_its_exact_unconfirmed_cancel_from_unknown_evidence() {
    let target = session("closing");
    let operation = OperationId::generate();
    let foreground = OperationId::generate();
    let other = OperationId::generate();
    let request = AdmissionRequest {
        operation: &operation,
        kind: ProviderOperationKind::ConversationClose,
        target: Some(&target),
        mode: AdmissionMode::HANDOFF,
    };
    let excluded = Some(foreground.as_str());
    assert!(!request.blocked_by(&foreground, EarlierOperation::Unknown, excluded));
    assert!(request.blocked_by(&other, EarlierOperation::Unknown, excluded));
    assert!(request.blocked_by(&foreground, EarlierOperation::Unknown, None));
}

#[test]
fn cancel_line_placement_and_reused_ids_keep_their_existing_boundaries() {
    let operation = OperationId::generate();
    let earlier = OperationId::generate();
    let target = session("foreground");
    let cancel = AdmissionRequest {
        operation: &operation,
        kind: ProviderOperationKind::ConversationCancel,
        target: Some(&target),
        mode: AdmissionMode::HANDOFF,
    };
    assert!(!cancel.blocked_by(&earlier, EarlierOperation::Unknown, None));
    assert!(!cancel.blocked_by(&earlier, EarlierOperation::InFlight { target: None }, None));

    let placement = AdmissionRequest {
        kind: ProviderOperationKind::ConversationPrompt,
        mode: AdmissionMode::deferred(AdmissionClass::LinePlacement),
        ..cancel
    };
    assert!(placement.blocked_by(&earlier, EarlierOperation::Unknown, None));
    assert!(!placement.blocked_by(&earlier, EarlierOperation::InFlight { target: None }, None));
    assert!(!placement.blocked_by(
        &earlier,
        EarlierOperation::InFlight {
            target: Some(&target)
        },
        None
    ));

    let reuse = AdmissionRequest {
        kind: ProviderOperationKind::ConversationLoad,
        ..cancel
    };
    assert!(!reuse.blocked_by(
        &operation,
        EarlierOperation::InFlight {
            target: Some(&target)
        },
        None
    ));
    assert!(reuse.blocked_by(
        &earlier,
        EarlierOperation::InFlight {
            target: Some(&target)
        },
        None
    ));
}
