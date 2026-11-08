//! The rules every machine-login read applies to an enrollment it finds:
//! idle expiry after 30 days without a Grant, and an approval's lapse after
//! the device-code lifetime. Central's own clock decides both.
use super::decision_support::{DecisionTime, MachineLoginSettings};
use super::enrollment_port::EnrollmentFacts;
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, InstallationRow, StoredEnrollment, StoredEnrollmentState,
};
use crate::identity_contract::AllowanceState;

/// An enrolled enrollment idle for the setting or longer (exactly 30 days
/// counts) expires: the installation loses its active enrollment and its
/// allowances are withdrawn. The generation doesn't move and no record is
/// written; no change kind exists for expiry.
pub(super) fn idle_expiry(
    enrollment: &StoredEnrollment,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    settings: &MachineLoginSettings,
) -> Option<Vec<EnrollmentChange>> {
    let StoredEnrollmentState::Enrolled {
        machine_client_id,
        last_grant_at,
    } = &enrollment.state
    else {
        return None;
    };
    if time.seconds().0.saturating_sub(last_grant_at.0) < settings.idle_expiry_seconds {
        return None;
    }
    let mut changes = vec![EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
        state: StoredEnrollmentState::Expired {
            machine_client_id: machine_client_id.clone(),
        },
        ..enrollment.clone()
    })];
    if let Some(row) = &facts.installation {
        if row.active.as_ref() == Some(&enrollment.id) {
            changes.push(EnrollmentChange::PutInstallation(InstallationRow {
                active: None,
                ..row.clone()
            }));
        }
    }
    changes.extend(
        facts
            .allowances
            .iter()
            .filter(|row| row.state == AllowanceState::Active)
            .map(|row| {
                EnrollmentChange::PutAllowance(AllowanceRow {
                    state: AllowanceState::Withdrawn,
                    ..row.clone()
                })
            }),
    );
    Some(changes)
}

/// An approval never granted within the device-code lifetime of its approval
/// lapses. Nothing else changes.
pub(super) fn approval_lapse(
    enrollment: &StoredEnrollment,
    time: DecisionTime,
    settings: &MachineLoginSettings,
) -> Option<EnrollmentChange> {
    let lapses_at = enrollment
        .approved_at_ms
        .saturating_add(settings.device_code_lifetime_ms);
    (enrollment.state == StoredEnrollmentState::Approved && time.now_ms >= lapses_at).then(|| {
        EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
            state: StoredEnrollmentState::Lapsed,
            ..enrollment.clone()
        })
    })
}
