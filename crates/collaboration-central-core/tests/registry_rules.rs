#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;
#[test]
fn renewal_replays_expiry_without_second_renewal_then_fresh_attempt_renews() {
    let mut h = Harness::setup();
    h.now = at(15);
    let first = h.renew(1, 1, 1);
    h.now = at(30);
    let replay = h.renew(1, 1, 1);
    match (first, replay) {
        (
            SessionAnswer::Renewed {
                expires_at: first,
                replayed: false,
                ..
            },
            SessionAnswer::Renewed {
                expires_at: replay,
                replayed: true,
                current,
                ..
            },
        ) => {
            assert_eq!(first, replay);
            assert_eq!(current.expires_at_registry, first);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(h.renew(1,2,1),SessionAnswer::Renewed{expires_at,replayed:false,..} if expires_at==at(270).0)
    );
}
#[test]
fn expiry_is_computed_on_supplied_clock_and_same_holder_recovers_same_epoch() {
    for minute in [0, 1, 239, 240, 241, 500] {
        let mut h = Harness::setup();
        h.now = at(minute);
        let result = h.renew(1, 1, 1);
        assert!(
            matches!(result,SessionAnswer::Renewed{current,..} if current.epoch==epoch(1)&&current.state==LeaseState::Held&&current.remaining_ms==14_400_000)
        );
    }
    let mut h = Harness::setup();
    h.renew(1, 1, 1);
    h.now = at(240);
    assert!(
        matches!(h.renew(1,1,1),SessionAnswer::Renewed{current,replayed:true,..} if current.state==LeaseState::Expired&&current.remaining_ms==0)
    );
}
#[test]
fn lost_renewal_replayed_after_takeover_reports_current_holder_then_fresh_old_attempt_stale() {
    let mut h = Harness::setup();
    h.renew(1, 1, 1);
    let call = h.takeover_call(1);
    assert!(
        matches!(h.call("registry",call),WireResult::Registry(RegistryResult::Admin(AdminResult::Granted{new_epoch,..})) if new_epoch==epoch(2))
    );
    h.now = at(15);
    assert!(
        matches!(h.renew(1,1,1),SessionAnswer::Renewed{replayed:true,current,..} if current.epoch==epoch(2)&&current.holder==machine(2))
    );
    assert_eq!(
        h.renew(1, 2, 1),
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(2),
            epoch_ended_at: position(1, 0)
        })
    );
    assert_eq!(
        h.renew(3, 3, 1),
        SessionAnswer::Refused(SessionRefusal::NotWriter {
            current: Some(epoch(2))
        })
    );
}
#[test]
fn changed_renewal_identity_is_refused_and_attempts_survive_time_without_cleanup() {
    let mut h = Harness::setup();
    h.renew(1, 1, 1);
    assert!(matches!(
        h.renew(2, 1, 1),
        SessionAnswer::Refused(SessionRefusal::ProjectRejected { .. })
    ));
    h.now = at(100_000);
    assert!(
        matches!(h.renew(1,1,1),SessionAnswer::Renewed{replayed:true,current,..} if current.state==LeaseState::Expired)
    );
}
#[test]
fn first_epoch_requires_fleet_acceptance_and_duplicate_creation_does_not_renew() {
    let mut h = Harness::new();
    h.enroll(1);
    assert_eq!(
        h.call(
            "registry",
            WireCall::Registry(RegistryCall::ReserveFirstEpoch {
                project: project(),
                source: ReservationSource::Machine {
                    installation: machine(1)
                }
            })
        ),
        WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::Refused(
            SessionRefusal::NotInitialized
        )))
    );
    let mut h = Harness::setup();
    h.now = at(15);
    assert!(
        matches!(h.call("registry",WireCall::Registry(RegistryCall::ReserveFirstEpoch{project:project(),source:ReservationSource::Machine{installation:machine(1)}})),WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::FirstLease{epoch:e,expires_at})) if e==epoch(1)&&expires_at==at(240).0)
    );
}
#[test]
fn admin_methods_reject_machine_and_agent_with_no_effect() {
    let mut h = Harness::setup();
    for authority in [
        AdminAuthority::Machine {
            installation: machine(1),
        },
        AdminAuthority::Agent,
    ] {
        assert_eq!(
            h.call(
                "registry",
                WireCall::Registry(RegistryCall::Admin {
                    authority,
                    request: AdminRequest::TakeoverPreview { project: project() }
                })
            ),
            WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
                AdminRefusal::AdminRequired
            )))
        );
    }
}

proptest::proptest! {
 #![proptest_config(proptest::test_runner::Config::with_cases(64))]
 #[test]
 fn renewal_replay_never_extends_expiry_for_any_elapsed_time(first_minute in 0i64..100_000, elapsed in 0i64..100_000) {
  let mut h=Harness::setup();h.now=at(first_minute);let original=h.renew(1,1,1);h.now=at(first_minute+elapsed);let replay=h.renew(1,1,1);
  let SessionAnswer::Renewed{expires_at:original,..}=original else {proptest::prop_assert!(false,"original renewal");return Ok(());};
  let SessionAnswer::Renewed{expires_at,replayed,current,..}=replay else {proptest::prop_assert!(false,"replayed renewal");return Ok(());};
  proptest::prop_assert!(replayed);proptest::prop_assert_eq!(expires_at,original);proptest::prop_assert_eq!(current.expires_at_registry,original);proptest::prop_assert_eq!(current.state,if elapsed<240{LeaseState::Held}else{LeaseState::Expired});
 }
 #[test]
 fn new_epoch_strictly_exceeds_every_proven_registry_floor(floor in 1u64..u64::MAX) {
  let mut h=Harness::setup();h.store("registry").connection.execute("UPDATE _registry_epoch_floor SET epoch=?",[floor.to_string()]).expect("floor");let call=h.takeover_call(1);let result=h.call("registry",call);proptest::prop_assert_eq!(result,WireResult::Registry(RegistryResult::Admin(AdminResult::Granted{new_epoch:epoch(floor+1),start:position(1,0)})));
 }
}
