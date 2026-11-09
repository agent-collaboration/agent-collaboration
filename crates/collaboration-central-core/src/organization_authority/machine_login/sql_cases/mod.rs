//! Every machine-login case the real SQL store must pass, run the same way on
//! both hosts: Tokio on sqlx-turso (core's own tests) and workerd on Durable
//! Object SQLite (`BootstrapObject`, fixture `machineLogin`). Each case starts
//! from a fresh store, drives the public decision entries, and checks the
//! store's own full readback: every row a step may change, with the exact
//! values it may change to, and nothing else.
//!
//! Fixture-only: the store is initialized with B3's staged, unregistered
//! corpus. Production bootstrap is unchanged. A case returns a failure instead
//! of panicking, so a refusal on workerd comes back as a readable answer.
mod case_values;
mod connection_cases;
mod constraint_cases;
mod conversion_cases;
mod decision_cases;
mod expected_rows;
mod record_cases;
mod sql_case_host;

pub use sql_case_host::run_sql_case;

use serde::{Deserialize, Serialize};

/// Every case, by name: both hosts run each one.
pub const SQL_CASES: &[&str] = &[
    "stagedCorpusInitializesOnce",
    "approvalAndFirstGrant",
    "repeatPendingOwner",
    "refusalsWriteNothing",
    "recordsKeepTheirOwnKeys",
    "firstGrantWins",
    "codeDeadlineLapses",
    "persistence",
    "failureAfterWritesRollsBack",
    "admissionReadsLive",
    "revocationEndsMachineAndEarlierApprovals",
    "convertedFormerIdLinksOwnMachineOnly",
    "engineRefusesSecondEnrolled",
    "engineRefusesDuplicateFormerId",
    "engineRefusesDuplicateAllowance",
    "engineRefusesDuplicateRequestOrClient",
    "invalidStoredRowsRefusedOnRead",
    "formerIdNeverReplaced",
    "todaysMachinesConvertOnce",
    "emptyFleetConverts",
    "refusalNamesEveryRefusedRow",
    "unfinishedLegacyRevocationRefuses",
    "unfinishedLegacyReadmissionRefuses",
    "failureAfterDestructiveStepRollsBack",
];

/// Which part of a case to run. Only `persistence` has two parts: `write`,
/// then `read` against the same persistence after the owner reopens it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CasePhase {
    Run,
    Write,
    Read,
}

/// Why a case failed: the step and what it observed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CaseFailure(pub String);
pub(crate) type CaseResult<T = ()> = Result<T, CaseFailure>;

impl From<durable_steps::StoreError> for CaseFailure {
    fn from(error: durable_steps::StoreError) -> Self {
        Self(format!("store: {error}"))
    }
}
impl From<super::enrollment_port::MachineEnrollmentFailure> for CaseFailure {
    fn from(failure: super::enrollment_port::MachineEnrollmentFailure) -> Self {
        Self(format!("decision: {failure:?}"))
    }
}

/// Fails the case unless both sides are equal.
macro_rules! check_eq {
    ($left:expr, $right:expr, $($context:tt)+) => {{
        let (left, right) = (&$left, &$right);
        if left != right {
            return Err($crate::organization_authority::machine_login::sql_cases::CaseFailure(
                format!("{}:\n  observed {:?}\n  expected {:?}", format_args!($($context)+), left, right),
            ));
        }
    }};
}
/// Fails the case unless the condition holds.
macro_rules! check {
    ($condition:expr, $($context:tt)+) => {{
        if !$condition {
            return Err($crate::organization_authority::machine_login::sql_cases::CaseFailure(
                format!($($context)+),
            ));
        }
    }};
}
pub(crate) use {check, check_eq};

/// Runs one case's body for its name: shared by both hosts' entries.
async fn dispatch<Host: sql_case_host::CaseHost>(
    name: &str,
    phase: CasePhase,
    host: &Host,
) -> CaseResult {
    if name != "persistence" && phase != CasePhase::Run {
        return Err(CaseFailure(format!("{name} has only the run phase")));
    }
    match name {
        "stagedCorpusInitializesOnce" => decision_cases::staged_corpus_initializes_once(host).await,
        "approvalAndFirstGrant" => decision_cases::approval_and_first_grant(host).await,
        "repeatPendingOwner" => decision_cases::repeat_pending_owner(host).await,
        "refusalsWriteNothing" => decision_cases::refusals_write_nothing(host).await,
        "recordsKeepTheirOwnKeys" => record_cases::records_keep_their_own_keys(host).await,
        "firstGrantWins" => decision_cases::first_grant_wins(host).await,
        "codeDeadlineLapses" => decision_cases::code_deadline_lapses(host).await,
        "persistence" => decision_cases::persistence(host, phase).await,
        "failureAfterWritesRollsBack" => {
            decision_cases::failure_after_writes_rolls_back(host).await
        }
        "admissionReadsLive" => connection_cases::admission_reads_live(host).await,
        "revocationEndsMachineAndEarlierApprovals" => {
            connection_cases::revocation_ends_machine_and_earlier_approvals(host).await
        }
        "convertedFormerIdLinksOwnMachineOnly" => {
            connection_cases::converted_former_id_links_own_machine_only(host).await
        }
        "engineRefusesSecondEnrolled" => constraint_cases::second_enrolled(host).await,
        "engineRefusesDuplicateFormerId" => constraint_cases::duplicate_former_id(host).await,
        "engineRefusesDuplicateAllowance" => constraint_cases::duplicate_allowance(host).await,
        "engineRefusesDuplicateRequestOrClient" => {
            constraint_cases::duplicate_request_or_client(host).await
        }
        "invalidStoredRowsRefusedOnRead" => constraint_cases::invalid_rows_refused(host).await,
        "formerIdNeverReplaced" => constraint_cases::former_id_never_replaced(host).await,
        "todaysMachinesConvertOnce" => conversion_cases::todays_machines_convert_once(host).await,
        "emptyFleetConverts" => conversion_cases::empty_fleet_converts(host).await,
        "refusalNamesEveryRefusedRow" => conversion_cases::refusal_names_every_row(host).await,
        "unfinishedLegacyRevocationRefuses" => {
            conversion_cases::unfinished_legacy_operation_refuses(host, true).await
        }
        "unfinishedLegacyReadmissionRefuses" => {
            conversion_cases::unfinished_legacy_operation_refuses(host, false).await
        }
        "failureAfterDestructiveStepRollsBack" => {
            conversion_cases::failure_after_destructive_step(host).await
        }
        unknown => Err(CaseFailure(format!(
            "no machine-login case is named {unknown}"
        ))),
    }
}

#[cfg(test)]
#[path = "sql_cases_test.rs"]
mod tests;
