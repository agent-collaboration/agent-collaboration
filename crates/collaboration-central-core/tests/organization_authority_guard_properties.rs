//! Generated pure-policy checks; these do not exercise a store or live issuer.

#[path = "support/person_token_fixture.rs"]
mod person_token_fixture;

use collaboration_central_core::{
    SignatureValidity,
    identity_contract::{
        CommandRefusal, OrganizationStanding, OwnerRefusal, PersonId, StandingRole,
    },
    organization_authority::{
        ConfirmedPersonToken, OwnerGuardSettings, PersonSessionConfirmation, PersonTokenAudience,
        authorize_owner_read, authorize_owner_write,
    },
};
use fleet_identity_protocol::{OrganizationId, UnixMillis, UnixSeconds};
use person_token_fixture::{
    ADMIN, NOW, ORGANIZATION, PERSON, SIGN_IN_SESSION, claims, person_token, policy,
};
use proptest::prelude::*;
use serde_json::json;

#[cfg(test)]
fn confirmed_at(received_at: UnixMillis, factor_at: Option<i64>) -> ConfirmedPersonToken {
    let issued_at = received_at.0.div_euclid(1_000);
    let expires_at = issued_at + 300;
    let mut body = claims();
    body.as_object_mut()
        .expect("controlled claims are an object")
        .extend([
            ("iat".into(), json!(issued_at)),
            ("exp".into(), json!(expires_at)),
            ("second_factor_at".into(), json!(factor_at)),
        ]);
    person_token(&body)
        .expect("controlled token parses")
        .verified(
            SignatureValidity::Valid,
            &policy().expect("controlled policy"),
            PersonTokenAudience::Admin,
            received_at,
        )
        .expect("controlled token follows policy")
        .confirmed(
            PersonSessionConfirmation::Active {
                person_id: PERSON.to_owned().try_into().expect("UUIDv7 person"),
                session_ref: SIGN_IN_SESSION
                    .to_owned()
                    .try_into()
                    .expect("UUIDv7 issuer session"),
                audience: ADMIN.into(),
                organization_id: ORGANIZATION
                    .to_owned()
                    .try_into()
                    .expect("UUIDv7 organization"),
                expires_at: UnixSeconds(expires_at),
            },
            received_at,
        )
        .expect("controlled confirmation matches the token")
}

fn owner_standing() -> OrganizationStanding {
    OrganizationStanding::Admitted {
        role: StandingRole::Owner,
    }
}

proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: 128,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0xf2a),
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    })]

    // Wrong behavior: rounding the receipt/apply times to seconds admits a
    // proof past SD8, or measuring from an absolute epoch refuses valid calls.
    // Oracle: PD R13/SD8, exactly five central-clock seconds, inclusive.
    #[test]
    fn organization_authority_guard_deadline_preserves_milliseconds_at_every_epoch(
        epoch_seconds in prop_oneof![Just(NOW.0 / 1_000), 1_000_000_i64..8_000_000_000_000],
        receipt_millis in 0_i64..1_000,
    ) {
        // Arrange: fresh factor and matching owner; all time is controlled.
        let received_at = UnixMillis(epoch_seconds * 1_000 + receipt_millis);
        let owner: PersonId = PERSON.to_owned().try_into().unwrap();
        let organization: OrganizationId = ORGANIZATION.to_owned().try_into().unwrap();
        let settings = OwnerGuardSettings::default();

        // Act / Assert: both callers accept the literal deadline, then refuse
        // one millisecond later. No subject read or store is mocked here.
        for (elapsed_millis, accepted) in [(5_000, true), (5_001, false)] {
            let applied_at = UnixMillis(received_at.0 + elapsed_millis);
            let read = authorize_owner_read(
                confirmed_at(received_at, Some(epoch_seconds - 50)),
                &organization, Some(&owner), &owner_standing(), applied_at, settings,
            );
            let write = authorize_owner_write(
                confirmed_at(received_at, Some(epoch_seconds - 50)),
                &organization, Some(&owner), &owner_standing(), applied_at, settings,
            );
            prop_assert_eq!(read.is_ok(), accepted);
            prop_assert_eq!(write.is_ok(), accepted);
            if !accepted {
                prop_assert!(matches!(read, Err(OwnerRefusal::NotOwner {})), "expired read proof");
                prop_assert!(matches!(write, Err(CommandRefusal::NotOwner {})), "expired write proof");
            }
        }
    }

    // Wrong behavior: a missing/replaced owner, removed member or member role
    // can read, write, or learn that the requested mutation needs a factor.
    // Oracle: R13 owner-first guard and R53's read exemption. The real token
    // reader/confirmation/guard run; transaction re-read remains separate proof.
    #[test]
    fn organization_authority_current_owner_precedes_every_factor_state(
        epoch_seconds in prop_oneof![Just(NOW.0 / 1_000), 1_000_000_i64..8_000_000_000_000],
        factor_age_seconds in 0_i64..7_200,
        factor_present in any::<bool>(),
    ) {
        // Arrange: a valid person proof, with an arbitrary factor age or none.
        let received_at = UnixMillis(epoch_seconds * 1_000);
        let factor_at = factor_present.then_some(epoch_seconds - factor_age_seconds);
        let owner: PersonId = PERSON.to_owned().try_into().unwrap();
        let other_owner: PersonId = ORGANIZATION.to_owned().try_into().unwrap();
        let organization: OrganizationId = ORGANIZATION.to_owned().try_into().unwrap();
        let settings = OwnerGuardSettings::default();

        // Act / Assert: a current owner's read is independent of factor age.
        let allowed_read = authorize_owner_read(
            confirmed_at(received_at, factor_at), &organization, Some(&owner),
            &owner_standing(), received_at, settings,
        );
        prop_assert!(allowed_read.is_ok(), "the current owner's read ignores factor freshness");

        for (current_owner, standing) in [
            (None, owner_standing()),
            (Some(&other_owner), owner_standing()),
            (Some(&owner), OrganizationStanding::NotAdmitted {}),
            (Some(&owner), OrganizationStanding::Admitted { role: StandingRole::Member }),
        ] {
            let read = authorize_owner_read(
                confirmed_at(received_at, factor_at), &organization, current_owner,
                &standing, received_at, settings,
            );
            let write = authorize_owner_write(
                confirmed_at(received_at, factor_at), &organization, current_owner,
                &standing, received_at, settings,
            );
            prop_assert!(matches!(read, Err(OwnerRefusal::NotOwner {})), "non-owner read");
            prop_assert!(matches!(write, Err(CommandRefusal::NotOwner {})), "non-owner write must reveal no factor hint");
        }
    }
}
