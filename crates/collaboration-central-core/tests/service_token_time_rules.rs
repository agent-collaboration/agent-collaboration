//! The Lead's C7 timing ruling is the oracle; no wall clock participates.
#![cfg(test)]

use collaboration_central_core::{
    ServiceTokenTimeRefusal, ServiceTokenTimeRule, validate_service_token_times,
};
use fleet_identity_protocol::UnixSeconds;

const NOW: i64 = 1_793_462_400;

#[test]
fn service_lifetime_has_no_clock_skew_extension() {
    assert_eq!(
        validate_service_token_times(
            ServiceTokenTimeRule::Service,
            UnixSeconds(NOW - 10),
            UnixSeconds(NOW + 290),
            UnixSeconds(NOW),
        ),
        Ok(())
    );
    assert_eq!(
        validate_service_token_times(
            ServiceTokenTimeRule::Service,
            UnixSeconds(NOW - 10),
            UnixSeconds(NOW + 291),
            UnixSeconds(NOW),
        ),
        Err(ServiceTokenTimeRefusal::LifetimeExceeded)
    );
}

#[test]
fn issuer_decision_lifetime_has_its_own_exact_bound() {
    assert_eq!(
        validate_service_token_times(
            ServiceTokenTimeRule::IssuerDecision,
            UnixSeconds(NOW - 10),
            UnixSeconds(NOW + 50),
            UnixSeconds(NOW),
        ),
        Ok(())
    );
    assert_eq!(
        validate_service_token_times(
            ServiceTokenTimeRule::IssuerDecision,
            UnixSeconds(NOW - 10),
            UnixSeconds(NOW + 51),
            UnixSeconds(NOW),
        ),
        Err(ServiceTokenTimeRefusal::LifetimeExceeded)
    );
}

#[test]
fn expiration_allows_skew_only_until_the_exact_deadline() {
    for rule in [
        ServiceTokenTimeRule::Service,
        ServiceTokenTimeRule::IssuerDecision,
    ] {
        assert_eq!(
            validate_service_token_times(
                rule,
                UnixSeconds(NOW - 60),
                UnixSeconds(NOW - 59),
                UnixSeconds(NOW),
            ),
            Ok(())
        );
        assert_eq!(
            validate_service_token_times(
                rule,
                UnixSeconds(NOW - 61),
                UnixSeconds(NOW - 60),
                UnixSeconds(NOW),
            ),
            Err(ServiceTokenTimeRefusal::Expired)
        );
    }
}

#[test]
fn issue_time_accepts_the_skew_boundary_and_refuses_the_next_second() {
    for rule in [
        ServiceTokenTimeRule::Service,
        ServiceTokenTimeRule::IssuerDecision,
    ] {
        assert_eq!(
            validate_service_token_times(
                rule,
                UnixSeconds(NOW + 60),
                UnixSeconds(NOW + 61),
                UnixSeconds(NOW),
            ),
            Ok(())
        );
        assert_eq!(
            validate_service_token_times(
                rule,
                UnixSeconds(NOW + 61),
                UnixSeconds(NOW + 62),
                UnixSeconds(NOW),
            ),
            Err(ServiceTokenTimeRefusal::NotYetValid)
        );
    }
}

#[test]
fn reversed_or_unrepresentable_intervals_are_refused() {
    for (issued, expires, now) in [
        (NOW + 1, NOW, NOW),
        (i64::MIN, i64::MAX, NOW),
        (i64::MAX - 1, i64::MAX, i64::MAX - 1),
    ] {
        assert_eq!(
            validate_service_token_times(
                ServiceTokenTimeRule::Service,
                UnixSeconds(issued),
                UnixSeconds(expires),
                UnixSeconds(now),
            ),
            Err(ServiceTokenTimeRefusal::InvalidInterval)
        );
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: 128,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0xc7),
        ..proptest::test_runner::Config::default()
    })]

    // Moving all three clocks by the same amount cannot change a rule whose
    // oracle is only signed duration and receiver/issuer clock differences.
    #[test]
    fn service_time_rules_are_invariant_under_a_shared_clock_shift(
        issued in 100_000_i64..200_000,
        lifetime in 0_i64..400,
        age in -200_i64..500,
        shift in -10_000_i64..10_000,
        issuer_decision in proptest::bool::ANY,
    ) {
        let rule = if issuer_decision {
            ServiceTokenTimeRule::IssuerDecision
        } else {
            ServiceTokenTimeRule::Service
        };
        let original = validate_service_token_times(
            rule,
            UnixSeconds(issued),
            UnixSeconds(issued + lifetime),
            UnixSeconds(issued + age),
        );
        let shifted = validate_service_token_times(
            rule,
            UnixSeconds(issued + shift),
            UnixSeconds(issued + lifetime + shift),
            UnixSeconds(issued + age + shift),
        );
        proptest::prop_assert_eq!(original, shifted);
    }
}
