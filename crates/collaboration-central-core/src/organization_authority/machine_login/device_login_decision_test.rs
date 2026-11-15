use super::super::{
    Decided, EnrollmentChange, EnrollmentGeneration, MachineRow, MachineState, StoredEnrollment,
    StoredEnrollmentState,
    policy_fixture::{
        CountingIds, NOW_MS, NOW_S, approval, approved, empty_facts, enrolled, id, label, machine,
        person, records, time, workspace,
    },
};
use super::{SD8_MILLIS, decide_device_login};
use crate::identity_contract::{
    DecideDeviceLogin, DeviceLoginDecision, DeviceLoginRefusal, EnrollmentRevocationReason,
};
use crate::organization_change_rows::{
    OrganizationChangeKind as Kind, OrganizationChangeOrigin as Origin,
};
use fleet_identity_protocol::UnixSeconds;

#[test]
fn approval_plans_an_inactive_snapshot_and_allowance_records_after_its_machine() {
    let request = approval();
    let ids = CountingIds::new();
    let plan = decide_device_login(&request, &empty_facts(), NOW_MS, time(NOW_MS), &ids).unwrap();
    let EnrollmentChange::PutMachine(machine) = &plan.changes[0] else {
        panic!("machine must precede approval")
    };
    let EnrollmentChange::InsertEnrollment(row) = &plan.changes[1] else {
        panic!("expected approval")
    };
    assert_eq!(machine.state, MachineState::Revoked);
    assert_eq!(machine.generation, EnrollmentGeneration::default());
    assert_eq!(row.machine_id, machine.id);
    assert_ne!(row.machine_id.to_string(), row.id.as_str());
    assert_eq!(row.state, StoredEnrollmentState::Approved);
    assert_eq!(row.approved_at_ms, NOW_MS);
    assert_eq!(row.code_expires_at, request.code_expires_at);
    assert_eq!(row.ended_at_ms, None);
    assert_eq!(
        plan.answer,
        DeviceLoginDecision::Approved {
            enrollment_id: row.id.clone(),
            label: request.label.clone(),
            allowed_workspaces: request.allowed_workspaces.clone(),
        }
    );
    for (record, workspace) in records(&plan)
        .into_iter()
        .zip(request.allowed_workspaces.as_slice())
    {
        assert_eq!(record.kind, Kind::AllowanceChanged);
        assert_eq!(record.target_id, workspace.as_str());
        assert_eq!(
            record.request_id.as_str(),
            format!(
                "{}:allowanceChanged:{}",
                request.request_id.as_str(),
                workspace.as_str()
            )
        );
        assert_eq!(
            record.origin,
            Origin::Person {
                person_id: person().as_str().to_owned(),
                permission_checked: "machine:enroll".to_owned()
            }
        );
        assert_eq!(record.created_at_millis, NOW_MS);
    }
    assert_eq!(records(&plan).len(), 2);
    assert_eq!(ids.minted(), 4);
}

#[test]
fn duplicate_allowed_workspaces_keep_the_snapshot_but_plan_one_allowance_and_record() {
    let duplicate_workspaces = vec![workspace(50), workspace(50)];
    let request = DecideDeviceLogin {
        allowed_workspaces: duplicate_workspaces.clone().try_into().unwrap(),
        ..approval()
    };
    let request: DecideDeviceLogin =
        serde_json::from_value(serde_json::to_value(request).unwrap()).unwrap();
    let ids = CountingIds::new();

    let plan = decide_device_login(&request, &empty_facts(), NOW_MS, time(NOW_MS), &ids).unwrap();

    let EnrollmentChange::PutMachine(machine) = &plan.changes[0] else {
        panic!("expected machine allowance union")
    };
    assert_eq!(machine.allowed_workspaces, vec![workspace(50)]);
    let EnrollmentChange::InsertEnrollment(enrollment) = &plan.changes[1] else {
        panic!("expected immutable approval")
    };
    assert_eq!(
        enrollment.allowed_workspaces.as_slice(),
        duplicate_workspaces.as_slice()
    );
    assert_eq!(
        plan.answer,
        DeviceLoginDecision::Approved {
            enrollment_id: enrollment.id.clone(),
            label: request.label.clone(),
            allowed_workspaces: request.allowed_workspaces.clone(),
        }
    );
    let audit = records(&plan);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].kind, Kind::AllowanceChanged);
    assert_eq!(audit[0].target_id, workspace(50).as_str());
    assert_eq!(
        audit[0].request_id.as_str(),
        format!(
            "{}:allowanceChanged:{}",
            request.request_id.as_str(),
            workspace(50).as_str()
        )
    );
    assert_eq!(plan.changes.len(), 3);
    assert_eq!(ids.minted(), 3);
}

#[test]
fn approving_again_preserves_the_machine_key_generation_active_state_and_existing_allowances() {
    for state in [MachineState::Enrolled, MachineState::Revoked] {
        let mut facts = empty_facts();
        let mut existing = machine(7);
        existing.state = state;
        existing.allowed_workspaces.push(workspace(52));
        facts.machine = Some(existing.clone());
        let mut request = approval();
        request.label = label("renamed");
        let plan = decide_device_login(&request, &facts, NOW_MS, time(NOW_MS), &CountingIds::new())
            .unwrap();
        let EnrollmentChange::PutMachine(updated) = &plan.changes[0] else {
            panic!("expected machine")
        };
        assert_eq!(
            updated,
            &MachineRow {
                label: label("renamed"),
                ..existing
            }
        );
        assert!(records(&plan).is_empty());
        let EnrollmentChange::InsertEnrollment(row) = &plan.changes[1] else {
            panic!("expected snapshot")
        };
        assert_eq!(row.allowed_workspaces, request.allowed_workspaces);
        assert_eq!(row.base_generation.get(), 7);
    }
}

#[test]
fn n03_returns_the_first_three_field_snapshot_after_mutable_inputs_and_state_change() {
    for state in [
        StoredEnrollmentState::Approved,
        StoredEnrollmentState::Lapsed,
        enrolled(40, 0, NOW_S).state,
        StoredEnrollmentState::Revoked {
            client_id: id(40).try_into().unwrap(),
            reason: EnrollmentRevocationReason::Owner {},
        },
        StoredEnrollmentState::Expired {
            client_id: id(40).try_into().unwrap(),
        },
    ] {
        let mut request = approval();
        request.label = label("different");
        request.allowed_workspaces = vec![workspace(99)].try_into().unwrap();
        request.approver_second_factor_at = None;
        request.code_expires_at = UnixSeconds(NOW_S - 1);
        let ended_at_ms = if matches!(
            state,
            StoredEnrollmentState::Approved | StoredEnrollmentState::Enrolled { .. }
        ) {
            None
        } else {
            Some(NOW_MS - 1)
        };
        let earlier = StoredEnrollment {
            request_id: request.request_id.clone(),
            ended_at_ms,
            state,
            ..approved(40, 0)
        };
        let mut facts = empty_facts();
        facts.organization_has_enrollment = true;
        facts.prior_approval = Some(earlier.clone());
        facts.missing_workspaces = vec![workspace(99)];
        let ids = CountingIds::new();
        let plan = decide_device_login(&request, &facts, NOW_MS, time(NOW_MS), &ids).unwrap();
        assert_eq!(
            plan,
            Decided::unchanged(DeviceLoginDecision::Approved {
                enrollment_id: earlier.id,
                label: earlier.label,
                allowed_workspaces: earlier.allowed_workspaces,
            })
        );
        assert_eq!(ids.minted(), 0);
    }
}

#[test]
fn sd8_org_and_peer_checks_precede_even_an_existing_approval() {
    let mut facts = empty_facts();
    facts.prior_approval = Some(StoredEnrollment {
        request_id: approval().request_id,
        ..approved(40, 0)
    });
    let ids = CountingIds::new();
    let within =
        decide_device_login(&approval(), &facts, NOW_MS - SD8_MILLIS, time(NOW_MS), &ids).unwrap();
    assert!(matches!(
        within.answer,
        DeviceLoginDecision::Approved { .. }
    ));
    let mut wrong_org = approval();
    wrong_org.organization_id = id(2).try_into().unwrap();
    let mut wrong_peer = approval();
    wrong_peer.peer = id(99).try_into().unwrap();
    for (request, received) in [
        (approval(), NOW_MS - SD8_MILLIS - 1),
        (wrong_org, NOW_MS),
        (wrong_peer, NOW_MS),
    ] {
        assert_eq!(
            decide_device_login(&request, &facts, received, time(NOW_MS), &ids).unwrap(),
            Decided::unchanged(DeviceLoginDecision::Refused {
                reason: DeviceLoginRefusal::NotPermitted {}
            })
        );
    }
    assert_eq!(ids.minted(), 0);
}

#[test]
fn retained_approval_refuses_a_different_installation_without_minting_or_mutation() {
    let original = approval();
    let earlier = StoredEnrollment {
        request_id: original.request_id.clone(),
        ..approved(40, 0)
    };
    let mut facts = empty_facts();
    facts.prior_approval = Some(earlier.clone());
    let mut another = original.clone();
    another.installation = id(99).try_into().unwrap();
    another.peer = id(99).try_into().unwrap();
    let ids = CountingIds::new();

    assert_eq!(
        decide_device_login(&another, &facts, NOW_MS, time(NOW_MS), &ids).unwrap(),
        Decided::unchanged(DeviceLoginDecision::Refused {
            reason: DeviceLoginRefusal::NotPermitted {}
        })
    );
    assert_eq!(
        decide_device_login(&original, &facts, NOW_MS, time(NOW_MS), &ids).unwrap(),
        Decided::unchanged(DeviceLoginDecision::Approved {
            enrollment_id: earlier.id,
            label: earlier.label,
            allowed_workspaces: earlier.allowed_workspaces,
        })
    );
    assert_eq!(ids.minted(), 0);
}

#[test]
fn new_approval_requires_a_deadline_strictly_after_the_applying_millisecond() {
    let request = DecideDeviceLogin {
        code_expires_at: UnixSeconds(NOW_S + 1),
        ..approval()
    };
    let deadline_ms = (NOW_S + 1) * 1000;
    for now_ms in [deadline_ms, deadline_ms + 1] {
        let ids = CountingIds::new();
        assert_eq!(
            decide_device_login(&request, &empty_facts(), now_ms, time(now_ms), &ids).unwrap(),
            Decided::unchanged(DeviceLoginDecision::Refused {
                reason: DeviceLoginRefusal::NotPermitted {}
            })
        );
        assert_eq!(ids.minted(), 0);
    }
    let ids = CountingIds::new();
    let before = deadline_ms - 1;
    let plan = decide_device_login(&request, &empty_facts(), before, time(before), &ids).unwrap();
    assert!(matches!(plan.answer, DeviceLoginDecision::Approved { .. }));
    assert!(!plan.changes.is_empty());
    assert!(ids.minted() > 0);
}

#[test]
fn overflowing_new_approval_deadline_is_not_permitted_without_ids_and_future_retry_approves() {
    let request = DecideDeviceLogin {
        code_expires_at: UnixSeconds(i64::MAX),
        ..approval()
    };
    let ids = CountingIds::new();
    assert_eq!(
        decide_device_login(&request, &empty_facts(), NOW_MS, time(NOW_MS), &ids),
        Ok(Decided::unchanged(DeviceLoginDecision::Refused {
            reason: DeviceLoginRefusal::NotPermitted {}
        }))
    );
    assert_eq!(ids.minted(), 0);

    let future = DecideDeviceLogin {
        code_expires_at: UnixSeconds(NOW_S + 1),
        ..request
    };
    let plan = decide_device_login(&future, &empty_facts(), NOW_MS, time(NOW_MS), &ids).unwrap();
    assert!(matches!(plan.answer, DeviceLoginDecision::Approved { .. }));
    assert!(!plan.changes.is_empty());
    assert!(ids.minted() > 0);
}

#[test]
fn first_machine_exemption_and_later_machine_factor_boundaries_are_preserved() {
    let mut facts = empty_facts();
    assert!(matches!(
        decide_device_login(
            &approval(),
            &facts,
            NOW_MS,
            time(NOW_MS),
            &CountingIds::new()
        )
        .unwrap()
        .answer,
        DeviceLoginDecision::Approved { .. }
    ));
    facts.organization_has_enrollment = true;
    let now_ms = NOW_S * 1000;
    let window = crate::person_request::SECOND_FACTOR_WINDOW_SECONDS;
    for factor in [None, Some(NOW_S - window - 1), Some(NOW_S + 1)] {
        let request = DecideDeviceLogin {
            approver_second_factor_at: factor.map(UnixSeconds),
            ..approval()
        };
        let plan = decide_device_login(&request, &facts, now_ms, time(now_ms), &CountingIds::new())
            .unwrap();
        assert_eq!(
            plan,
            Decided::unchanged(DeviceLoginDecision::Refused {
                reason: DeviceLoginRefusal::SecondFactorRequired {
                    within_seconds: u32::try_from(window).unwrap().try_into().unwrap()
                },
            })
        );
    }
    let request = DecideDeviceLogin {
        approver_second_factor_at: Some(UnixSeconds(NOW_S - window)),
        ..approval()
    };
    assert!(matches!(
        decide_device_login(&request, &facts, now_ms, time(now_ms), &CountingIds::new())
            .unwrap()
            .answer,
        DeviceLoginDecision::Approved { .. }
    ));
}

#[test]
fn unknown_workspace_refuses_without_a_plan_or_minted_ids() {
    let mut facts = empty_facts();
    facts.missing_workspaces = vec![workspace(50)];
    let ids = CountingIds::new();
    assert_eq!(
        decide_device_login(&approval(), &facts, NOW_MS, time(NOW_MS), &ids).unwrap(),
        Decided::unchanged(DeviceLoginDecision::Refused {
            reason: DeviceLoginRefusal::UnknownWorkspace {}
        })
    );
    assert_eq!(ids.minted(), 0);
}
