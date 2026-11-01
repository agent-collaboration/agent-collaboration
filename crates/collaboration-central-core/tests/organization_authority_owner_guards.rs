//! Pure R13/R53 decisions; the transaction interleaving remains a both-host proof.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail guard regressions while fixture errors propagate."
)]

#[path = "support/person_token_fixture.rs"]
mod person_token_fixture;

use collaboration_central_core::{
    SignatureValidity,
    identity_contract::{
        CommandRefusal, OrganizationStanding, OwnerRefusal, PersonId, StandingRole,
    },
    organization_authority::{
        ConfirmedPersonToken, OwnerGuardSettings, PersonSessionConfirmation, PersonTokenAudience,
        authorize_operator, authorize_owner_read, authorize_owner_write,
    },
};
use fleet_identity_protocol::{OrganizationId, UnixMillis, UnixSeconds};
use person_token_fixture::{
    ADMIN, NOW, OPERATOR, ORGANIZATION, PERSON, claims, person_token, policy,
};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn confirmed(
    purpose: PersonTokenAudience,
    factor_at: Option<i64>,
) -> Result<ConfirmedPersonToken, Box<dyn std::error::Error + Send + Sync>> {
    confirmed_with_expiry(purpose, factor_at, 1_800_000_300)
}

fn confirmed_with_expiry(
    purpose: PersonTokenAudience,
    factor_at: Option<i64>,
    expires_at: i64,
) -> Result<ConfirmedPersonToken, Box<dyn std::error::Error + Send + Sync>> {
    let mut body = claims();
    let audience = match purpose {
        PersonTokenAudience::Admin => ADMIN,
        PersonTokenAudience::Operator => OPERATOR,
    };
    body.as_object_mut()
        .ok_or("person claims must be an object")?
        .extend([
            ("aud".into(), json!(audience)),
            ("second_factor_at".into(), json!(factor_at)),
            ("exp".into(), json!(expires_at)),
            ("iat".into(), json!(expires_at.saturating_sub(300))),
        ]);
    Ok(person_token(&body)?
        .verified(SignatureValidity::Valid, &policy()?, purpose, NOW)?
        .confirmed(
            PersonSessionConfirmation::Active {
                person_id: PERSON.to_owned().try_into()?,
                session_ref: "issuer-owned-session".to_owned().try_into()?,
                audience: audience.into(),
                organization_id: ORGANIZATION.to_owned().try_into()?,
                expires_at: UnixSeconds(expires_at),
            },
            NOW,
        )?)
}

fn owner_standing() -> OrganizationStanding {
    OrganizationStanding::Admitted {
        role: StandingRole::Owner,
    }
}

#[test]
fn organization_authority_owner_reads_do_not_require_a_second_factor() -> TestResult {
    let organization: OrganizationId = ORGANIZATION.to_owned().try_into()?;
    let owner: PersonId = PERSON.to_owned().try_into()?;
    let actor = authorize_owner_read(
        confirmed(PersonTokenAudience::Admin, None)?,
        &organization,
        Some(&owner),
        &owner_standing(),
        NOW,
        OwnerGuardSettings::default(),
    )
    .map_err(|refusal| format!("owner read was refused: {refusal:?}"))?;
    assert_eq!(actor.person_id(), &owner);
    Ok(())
}

#[test]
fn organization_authority_owner_writes_require_a_factor_within_fifteen_minutes() -> TestResult {
    let organization: OrganizationId = ORGANIZATION.to_owned().try_into()?;
    let owner: PersonId = PERSON.to_owned().try_into()?;
    let settings = OwnerGuardSettings::default();
    authorize_owner_write(
        confirmed(PersonTokenAudience::Admin, Some(1_799_999_100))?,
        &organization,
        Some(&owner),
        &owner_standing(),
        NOW,
        settings,
    )
    .map_err(|refusal| format!("in-window owner write was refused: {refusal:?}"))?;
    for (factor, now) in [
        (None, NOW),
        (Some(1_799_999_100), UnixMillis(1_800_000_000_001)),
    ] {
        assert!(matches!(
            authorize_owner_write(
                confirmed(PersonTokenAudience::Admin, factor)?,
                &organization,
                Some(&owner),
                &owner_standing(),
                now,
                settings
            ),
            Err(CommandRefusal::SecondFactorRequired {
                within_seconds: 900
            })
        ));
    }
    Ok(())
}

#[test]
fn organization_authority_non_owner_never_receives_a_step_up_hint() -> TestResult {
    let organization: OrganizationId = ORGANIZATION.to_owned().try_into()?;
    let other_owner: PersonId = ORGANIZATION.to_owned().try_into()?;
    assert!(matches!(
        authorize_owner_write(
            confirmed(PersonTokenAudience::Admin, None)?,
            &organization,
            Some(&other_owner),
            &owner_standing(),
            NOW,
            OwnerGuardSettings::default()
        ),
        Err(CommandRefusal::NotOwner {})
    ));
    Ok(())
}

#[test]
fn organization_authority_removed_member_other_org_and_operator_proofs_cannot_act_as_owner()
-> TestResult {
    let organization: OrganizationId = ORGANIZATION.to_owned().try_into()?;
    let wrong_organization: OrganizationId = PERSON.to_owned().try_into()?;
    let owner: PersonId = PERSON.to_owned().try_into()?;
    for (target, standing, purpose) in [
        (
            &organization,
            OrganizationStanding::NotAdmitted {},
            PersonTokenAudience::Admin,
        ),
        (
            &organization,
            OrganizationStanding::Admitted {
                role: StandingRole::Member,
            },
            PersonTokenAudience::Admin,
        ),
        (
            &wrong_organization,
            owner_standing(),
            PersonTokenAudience::Admin,
        ),
        (
            &organization,
            owner_standing(),
            PersonTokenAudience::Operator,
        ),
    ] {
        assert!(matches!(
            authorize_owner_read(
                confirmed(purpose, None)?,
                target,
                Some(&owner),
                &standing,
                NOW,
                OwnerGuardSettings::default()
            ),
            Err(OwnerRefusal::NotOwner {})
        ));
    }
    Ok(())
}

#[test]
fn organization_authority_operator_authority_is_separate_from_organization_ownership() -> TestResult
{
    let operator: PersonId = PERSON.to_owned().try_into()?;
    let other_person: PersonId = ORGANIZATION.to_owned().try_into()?;
    authorize_operator(
        confirmed(PersonTokenAudience::Operator, None)?,
        &operator,
        NOW,
        OwnerGuardSettings::default(),
    )
    .map_err(|refusal| format!("configured operator was refused: {refusal:?}"))?;
    for (purpose, configured_operator) in [
        (PersonTokenAudience::Admin, &operator),
        (PersonTokenAudience::Operator, &other_person),
    ] {
        assert!(matches!(
            authorize_operator(
                confirmed(purpose, None)?,
                configured_operator,
                NOW,
                OwnerGuardSettings::default()
            ),
            Err(CommandRefusal::NotOwner {})
        ));
    }
    Ok(())
}

#[test]
fn organization_authority_no_owner_binding_or_stale_confirmation_refuses_a_read() -> TestResult {
    let organization: OrganizationId = ORGANIZATION.to_owned().try_into()?;
    let owner: PersonId = PERSON.to_owned().try_into()?;
    for (current_owner, now) in [(None, NOW), (Some(&owner), UnixMillis(1_800_000_005_001))] {
        assert!(matches!(
            authorize_owner_read(
                confirmed(PersonTokenAudience::Admin, None)?,
                &organization,
                current_owner,
                &owner_standing(),
                now,
                OwnerGuardSettings::default()
            ),
            Err(OwnerRefusal::NotOwner {})
        ));
    }
    Ok(())
}

#[test]
fn organization_authority_apply_rechecks_token_expiry_inside_a_fresh_confirmation_window()
-> TestResult {
    let organization: OrganizationId = ORGANIZATION.to_owned().try_into()?;
    let owner: PersonId = PERSON.to_owned().try_into()?;
    assert!(matches!(
        authorize_owner_read(
            confirmed_with_expiry(PersonTokenAudience::Admin, None, 1_799_999_941)?,
            &organization,
            Some(&owner),
            &owner_standing(),
            UnixMillis(1_800_000_001_000),
            OwnerGuardSettings::default(),
        ),
        Err(OwnerRefusal::NotOwner {})
    ));
    Ok(())
}
