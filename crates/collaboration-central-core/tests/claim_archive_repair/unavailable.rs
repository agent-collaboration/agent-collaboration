use super::*;

#[test]
fn unavailable_claim_restore_probe_keeps_active_reservation_until_actual_refusal() {
    let mut fixture = ClaimArchiveFixture::new();
    let project = fixture.project.clone();
    fixture.harness.call(
        "copy",
        WireCall::Copy(CopyCall::Fence {
            transfer_id: transfer(3_070),
            new_epoch: epoch(2),
            holder: machine(2),
            expect: FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 0),
            },
            target_schema_max: 10,
        }),
    );
    let claim = fixture.claim(3_071);
    let (_, effects) = fixture.harness.begin("registry", claim.clone());
    let reserve = only_fleet(&effects);
    let reserved = reserve.answer(&mut fixture.harness);
    let fence = reserve.deliver(&mut fixture.harness, reserved);
    let probe = fixture.copy_call(&fence, true);
    let call_id = probe
        .iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                call: WireCall::Copy(CopyCall::CopyStatus { project: actual }),
                ..
            } if actual == &project => Some(call_id.clone()),
            _ => None,
        })
        .expect("restore status probe");
    let before_transfer = fixture.transfer(3_071);
    let before_reservation = fixture.reservation(3_071);
    assert_eq!(before_reservation.state, ClaimReservationState::Active);
    let effects = fixture.harness.run(
        "registry",
        WireInput::CallResult {
            source: CallerKind::Registry,
            call_id,
            carry: None,
            result: Box::new(WireResult::Copy(CopyResult::Unavailable(
                CopyUnavailableReason::StorageFull,
            ))),
        },
    );
    assert!(matches!(effects.as_slice(), [WireEffect::Return {
        result: WireResult::Registry(RegistryResult::Claim(SessionAnswer::Unavailable {
            subject: BlockedSubject::Transfer { transfer }, ..
        })), ..
    }] if transfer.as_str() == request(3_071).as_str()));
    assert_eq!(fixture.transfer(3_071), before_transfer);
    assert_eq!(fixture.reservation(3_071), before_reservation);
    fixture.harness.restart_registry();
    assert_eq!(
        fixture.harness.call("registry", claim),
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Refused(
            SessionRefusal::AlreadyHeld { holder: machine(2) }
        )))
    );
    assert_eq!(
        fixture.reservation(3_071).state,
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Refused,
        }
    );
}

#[test]
fn unavailable_claim_fence_keeps_active_fleet_reservation_and_replays_after_repair() {
    for reason in [
        CopyUnavailableReason::StorageFull,
        CopyUnavailableReason::MigrationPending {
            kind: BookkeepingKind::Copy,
            version: 3,
        },
    ] {
        for committed in [false, true] {
            let mut fixture = ClaimArchiveFixture::new();
            let claim = fixture.claim(3_050);
            let (_, effects) = fixture.harness.begin("registry", claim.clone());
            let reserve = only_fleet(&effects);
            let reserved = reserve.answer(&mut fixture.harness);
            let fence = reserve.deliver(&mut fixture.harness, reserved);
            if committed {
                fixture.copy_call(&fence, false);
            }
            let before_transfer = fixture.transfer(3_050);
            let before_reservation = fixture.reservation(3_050);
            assert!(
                matches!(before_transfer.state, TransferState::Fencing { new_epoch, .. } if new_epoch == epoch(2))
            );
            assert_eq!(before_reservation.state, ClaimReservationState::Active);
            let call_id = fence
                .iter()
                .find_map(|effect| match effect {
                    WireEffect::Dispatch {
                        call_id,
                        call: WireCall::Copy(_),
                        ..
                    } => Some(call_id.clone()),
                    _ => None,
                })
                .expect("fence continuation");
            let effects = fixture.harness.run(
                "registry",
                WireInput::CallResult {
                    source: CallerKind::Registry,
                    call_id,
                    carry: None,
                    result: Box::new(WireResult::Copy(CopyResult::Unavailable(reason.clone()))),
                },
            );
            let expected_transfer =
                TransferId::new(request(3_050).as_str()).expect("existing attempt identity");
            assert!(matches!(effects.as_slice(), [WireEffect::Return {
                result: WireResult::Registry(RegistryResult::Claim(SessionAnswer::Unavailable {
                    subject: BlockedSubject::Transfer { transfer }, reason: actual,
                })), ..
            }] if transfer == &expected_transfer && actual == &reason));
            assert_eq!(fixture.transfer(3_050), before_transfer);
            assert_eq!(fixture.reservation(3_050), before_reservation);
            fixture.harness.restart_registry();
            for _ in 0..2 {
                assert!(matches!(fixture.harness.call("registry", claim.clone()),
                    WireResult::Registry(RegistryResult::Claim(SessionAnswer::Claimed { epoch: granted, start, .. }))
                    if granted == epoch(2) && start == position(1, 0)));
            }
            assert_eq!(
                fixture.reservation(3_050).state,
                ClaimReservationState::Settled {
                    outcome: ClaimReservationOutcome::Granted,
                }
            );
        }
    }
}

#[test]
fn unavailable_claim_schema_probe_keeps_the_attempt_retryable_without_recording_a_rejection() {
    let mut fixture = ClaimArchiveFixture::new();
    fixture.harness.enroll(3); // No advertised schema support yet.
    let claim = WireCall::Registry(RegistryCall::Claim {
        attempt: request(3_060),
        project: fixture.project.clone(),
        claimant: machine(3),
    });
    let (_, effects) = fixture.harness.begin("registry", claim.clone());
    let call_id = effects
        .iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                call: WireCall::Copy(CopyCall::CopyStatus { .. }),
                ..
            } => Some(call_id.clone()),
            _ => None,
        })
        .expect("schema probe");
    let effects = fixture.harness.run(
        "registry",
        WireInput::CallResult {
            source: CallerKind::Registry,
            call_id,
            carry: None,
            result: Box::new(WireResult::Copy(CopyResult::Unavailable(
                CopyUnavailableReason::StorageFull,
            ))),
        },
    );
    assert!(matches!(effects.as_slice(), [WireEffect::Return {
        result: WireResult::Registry(RegistryResult::Claim(SessionAnswer::Unavailable {
            subject: BlockedSubject::Transfer { transfer }, reason: CopyUnavailableReason::StorageFull,
        })), ..
    }] if transfer.as_str() == request(3_060).as_str()));
    let rejections: u64 = fixture
        .harness
        .store("registry")
        .connection
        .query_row(
            "SELECT count(*) FROM _registry_claim_rejections",
            [],
            |row| row.get(0),
        )
        .expect("rejections");
    assert_eq!(rejections, 0);
    fixture.harness.register(3, 10);
    assert!(matches!(fixture.harness.call("registry", claim),
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Claimed { epoch: granted, .. })) if granted == epoch(2)));
}
