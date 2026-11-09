//! The rules every machine-login read applies to an enrollment it finds:
//! idle expiry after 30 days without a Grant, and an approval's lapse at its
//! device code's deadline. Central's own clock decides both.
use super::decision_support::{DecisionIds, DecisionTime, MachineLoginSettings};
use super::enrollment_port::{EnrollmentFacts, MachineEnrollmentFailure};
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, InstallationRow, StoredEnrollment, StoredEnrollmentState,
};
use crate::identity_contract::AllowanceState;
use crate::organization_authority::{OrganizationChangeKind, OrganizationChangeOrigin};

/// An enrolled enrollment idle for the setting or longer (exactly 30 days
/// counts) expires: the installation loses its active enrollment and its
/// allowances are withdrawn. The generation doesn't move. The decision that
/// first observes it records `machineExpired` and each withdrawal, as
/// automatic changes that name no person (owner decision N07).
pub(super) fn idle_expiry(
    enrollment: &StoredEnrollment,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    settings: &MachineLoginSettings,
    ids: &DecisionIds<'_>,
) -> Result<Option<Vec<EnrollmentChange>>, MachineEnrollmentFailure> {
    let StoredEnrollmentState::Enrolled {
        machine_client_id,
        last_grant_at,
    } = &enrollment.state
    else {
        return Ok(None);
    };
    if time.seconds().0.saturating_sub(last_grant_at.0) < settings.idle_expiry_seconds {
        return Ok(None);
    }
    let originating = enrollment.id.as_str();
    let automatic = OrganizationChangeOrigin::Automatic {};
    let mut changes = vec![
        EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
            state: StoredEnrollmentState::Expired {
                machine_client_id: machine_client_id.clone(),
            },
            ended_at_ms: Some(time.now_ms),
            ..enrollment.clone()
        }),
        EnrollmentChange::Record(ids.record(
            originating,
            OrganizationChangeKind::MachineExpired,
            &automatic,
            enrollment.id.as_str().to_owned(),
            time,
        )?),
    ];
    if let Some(row) = &facts.installation
        && row.active.as_ref() == Some(&enrollment.id)
    {
        changes.push(EnrollmentChange::PutInstallation(InstallationRow {
            active: None,
            ..row.clone()
        }));
    }
    for allowance in facts
        .allowances
        .iter()
        .filter(|row| row.state == AllowanceState::Active)
    {
        changes.push(EnrollmentChange::PutAllowance(AllowanceRow {
            state: AllowanceState::Withdrawn,
            ..allowance.clone()
        }));
        changes.push(EnrollmentChange::Record(ids.record(
            originating,
            OrganizationChangeKind::AllowanceChanged,
            &automatic,
            allowance.id.to_string(),
            time,
        )?));
    }
    Ok(Some(changes))
}

/// An approval never granted by its device code's deadline lapses: at the
/// deadline itself, and after (owner decision N10). Nothing else changes.
pub(super) fn approval_lapse(
    enrollment: &StoredEnrollment,
    time: DecisionTime,
) -> Option<EnrollmentChange> {
    let lapses_at_ms = enrollment.code_expires_at.0.saturating_mul(1000);
    (enrollment.state == StoredEnrollmentState::Approved && time.now_ms >= lapses_at_ms).then(
        || {
            EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
                state: StoredEnrollmentState::Lapsed,
                ended_at_ms: Some(time.now_ms),
                ..enrollment.clone()
            })
        },
    )
}
