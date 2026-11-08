//! Stand-in for an organization's Fleet store behind `MachineEnrollmentPort`,
//! until the SQL lands. It applies a decision's changes to a copy, checks the
//! port's invariants against the state before, and swaps the copy in only if
//! they hold: one transaction, all or nothing.
use super::enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, EnrollmentGeneration, FormerServiceFacts, InstallationRow,
    MachineClaim, StoredEnrollment, StoredEnrollmentState,
};
use crate::identity_contract::{EnrollmentRevocationReason, FormerServiceId};
use crate::organization_authority::OrganizationChangeRecord;
use fleet_identity_protocol::{EnrollmentId, InstallationId};
use message_board::WorkspaceId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// Everything the stand-in holds; tests compare it before and after.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EnrollmentStore {
    pub enrollments: BTreeMap<EnrollmentId, StoredEnrollment>,
    pub installations: BTreeMap<InstallationId, InstallationRow>,
    pub allowances: Vec<AllowanceRow>,
    pub links: Vec<(FormerServiceId, InstallationId)>,
    pub records: Vec<OrganizationChangeRecord>,
    pub workspaces: BTreeSet<WorkspaceId>,
    /// Installation and peer IDs another organization holds.
    pub claimed_elsewhere: BTreeSet<String>,
}

type BeforeDecision = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub(crate) struct InMemoryEnrollments {
    store: Mutex<EnrollmentStore>,
    /// Runs inside the next transaction before its decision: a wait the
    /// decision must sample its clock after.
    before_decision: Mutex<Option<BeforeDecision>>,
}

impl InMemoryEnrollments {
    pub(crate) fn with(store: EnrollmentStore) -> Self {
        Self {
            store: Mutex::new(store),
            before_decision: Mutex::new(None),
        }
    }

    pub(crate) fn snapshot(&self) -> EnrollmentStore {
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn before_next_decision(&self, hook: impl FnOnce() + Send + 'static) {
        *self
            .before_decision
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(hook));
    }
}

impl MachineEnrollmentPort for InMemoryEnrollments {
    async fn transact<T: Send>(
        &self,
        query: EnrollmentQuery,
        decide: impl FnOnce(&EnrollmentFacts) -> Decided<T> + Send,
    ) -> Result<T, MachineEnrollmentFailure> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| MachineEnrollmentFailure::Unavailable)?;
        let hook = self
            .before_decision
            .lock()
            .map_err(|_| MachineEnrollmentFailure::Unavailable)?
            .take();
        if let Some(hook) = hook {
            hook();
        }
        let decided = decide(&facts(&store, &query));
        let mut next = store.clone();
        for change in &decided.changes {
            apply(&mut next, change)?;
        }
        check_invariants(&store, &next, &decided.changes)?;
        *store = next;
        Ok(decided.answer)
    }
}

fn facts(store: &EnrollmentStore, query: &EnrollmentQuery) -> EnrollmentFacts {
    let by_request = query.request.as_ref().and_then(|request| {
        store
            .enrollments
            .values()
            .find(|enrollment| &enrollment.approval.request_id == request)
            .cloned()
    });
    let enrollment = query
        .enrollment
        .as_ref()
        .and_then(|id| store.enrollments.get(id).cloned());
    let installation_key = query
        .installation
        .clone()
        .or_else(|| enrollment.as_ref().map(|found| found.installation.clone()));
    let installation = installation_key
        .as_ref()
        .and_then(|key| store.installations.get(key).cloned());
    let active = installation
        .as_ref()
        .and_then(|row| row.active.as_ref())
        .and_then(|id| store.enrollments.get(id).cloned());
    let allowances = installation_key.as_ref().map_or_else(Vec::new, |key| {
        store
            .allowances
            .iter()
            .filter(|allowance| &allowance.installation == key)
            .cloned()
            .collect()
    });
    let held_elsewhere = query
        .installation
        .as_ref()
        .is_some_and(|id| store.claimed_elsewhere.contains(id.as_str()))
        || query
            .peer
            .as_ref()
            .is_some_and(|peer| store.claimed_elsewhere.contains(&peer.to_string()));
    let claim = if held_elsewhere {
        MachineClaim::AnotherOrganization
    } else if installation.is_some() {
        MachineClaim::ThisOrganization
    } else {
        MachineClaim::Unclaimed
    };
    let former = query
        .former
        .as_ref()
        .map_or(FormerServiceFacts::NotAsked, |former| {
            former_facts(store, former)
        });
    EnrollmentFacts {
        organization_has_enrollment: !store.enrollments.is_empty(),
        by_request,
        enrollment,
        installation,
        active,
        allowances,
        claim,
        missing_workspaces: query
            .workspaces
            .iter()
            .filter(|workspace| !store.workspaces.contains(*workspace))
            .cloned()
            .collect(),
        former,
    }
}

fn former_facts(store: &EnrollmentStore, former: &FormerServiceId) -> FormerServiceFacts {
    if let Some((_, installation)) = store.links.iter().find(|(linked, _)| linked == former) {
        return FormerServiceFacts::LinkedTo {
            installation: installation.clone(),
        };
    }
    match InstallationId::try_from(String::from(former.clone())) {
        Ok(installation) => match store.installations.get(&installation) {
            Some(row) => FormerServiceFacts::Unconverted {
                row: row.clone(),
                allowances: store
                    .allowances
                    .iter()
                    .filter(|allowance| allowance.installation == installation)
                    .cloned()
                    .collect(),
            },
            None => FormerServiceFacts::NamesNothing,
        },
        _ => FormerServiceFacts::NamesNothing,
    }
}

fn apply(
    store: &mut EnrollmentStore,
    change: &EnrollmentChange,
) -> Result<(), MachineEnrollmentFailure> {
    match change {
        EnrollmentChange::InsertEnrollment(enrollment) => {
            if store
                .enrollments
                .insert(enrollment.id.clone(), enrollment.clone())
                .is_some()
            {
                return Err(violated("an inserted enrollment already exists"));
            }
        }
        EnrollmentChange::ReplaceEnrollment(enrollment) => {
            let Some(existing) = store.enrollments.get_mut(&enrollment.id) else {
                return Err(violated("a replaced enrollment does not exist"));
            };
            *existing = enrollment.clone();
        }
        EnrollmentChange::PutInstallation(row) => {
            store
                .installations
                .insert(row.installation.clone(), row.clone());
        }
        EnrollmentChange::PutAllowance(allowance) => {
            match store
                .allowances
                .iter_mut()
                .find(|row| row.id == allowance.id)
            {
                Some(row) => *row = allowance.clone(),
                None => store.allowances.push(allowance.clone()),
            }
        }
        EnrollmentChange::RekeyInstallation { from, to } => rekey(store, from, to)?,
        EnrollmentChange::LinkFormerService {
            former,
            installation,
        } => {
            if store.links.iter().any(|(linked, _)| linked == former) {
                return Err(violated("a former service id is linked at most once"));
            }
            store.links.push((former.clone(), installation.clone()));
        }
        EnrollmentChange::Record(record) => store.records.push(record.clone()),
    }
    Ok(())
}

fn rekey(
    store: &mut EnrollmentStore,
    from: &InstallationId,
    to: &InstallationId,
) -> Result<(), MachineEnrollmentFailure> {
    if store.installations.contains_key(to) {
        return Err(violated("a re-key target installation already exists"));
    }
    if let Some(mut row) = store.installations.remove(from) {
        row.installation = to.clone();
        store.installations.insert(to.clone(), row);
    }
    for enrollment in store.enrollments.values_mut() {
        if &enrollment.installation == from {
            enrollment.installation = to.clone();
        }
    }
    for allowance in &mut store.allowances {
        if &allowance.installation == from {
            allowance.installation = to.clone();
        }
    }
    Ok(())
}

fn check_invariants(
    before: &EnrollmentStore,
    after: &EnrollmentStore,
    changes: &[EnrollmentChange],
) -> Result<(), MachineEnrollmentFailure> {
    for row in after.installations.values() {
        let enrolled = after
            .enrollments
            .values()
            .filter(|enrollment| {
                enrollment.installation == row.installation
                    && matches!(enrollment.state, StoredEnrollmentState::Enrolled { .. })
            })
            .map(|enrollment| &enrollment.id)
            .collect::<Vec<_>>();
        if enrolled.len() > 1 {
            return Err(violated(
                "at most one enrollment per installation is enrolled",
            ));
        }
        if enrolled.first().copied() != row.active.as_ref() {
            return Err(violated("the active enrollment is the enrolled one"));
        }
        let previous = previous_generation(before, changes, &row.installation);
        if row.generation != previous {
            let fenced = row.generation == previous.next()
                && moves_generation(before, changes, &row.installation);
            if !fenced {
                return Err(violated(
                    "the generation moves by one, only at a first Grant or an owner revocation",
                ));
            }
        }
    }
    Ok(())
}

/// The installation's generation before, following a re-key back to its
/// former key.
fn previous_generation(
    before: &EnrollmentStore,
    changes: &[EnrollmentChange],
    installation: &InstallationId,
) -> EnrollmentGeneration {
    let former = changes.iter().find_map(|change| match change {
        EnrollmentChange::RekeyInstallation { from, to } if to == installation => Some(from),
        _ => None,
    });
    before
        .installations
        .get(former.unwrap_or(installation))
        .map_or_else(EnrollmentGeneration::default, |row| row.generation)
}

fn moves_generation(
    before: &EnrollmentStore,
    changes: &[EnrollmentChange],
    installation: &InstallationId,
) -> bool {
    changes.iter().any(|change| {
        let EnrollmentChange::ReplaceEnrollment(next) = change else {
            return false;
        };
        let Some(previous) = before.enrollments.get(&next.id) else {
            return false;
        };
        &next.installation == installation
            && matches!(
                (&previous.state, &next.state),
                (
                    StoredEnrollmentState::Approved,
                    StoredEnrollmentState::Enrolled { .. }
                ) | (
                    StoredEnrollmentState::Enrolled { .. },
                    StoredEnrollmentState::Revoked {
                        reason: EnrollmentRevocationReason::Owner {},
                        ..
                    }
                )
            )
    })
}

const fn violated(rule: &'static str) -> MachineEnrollmentFailure {
    MachineEnrollmentFailure::ContractViolated { rule }
}
