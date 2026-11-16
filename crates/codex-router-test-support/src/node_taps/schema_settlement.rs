//! How a case's recorded messages stand against its listed schema defects (R36, R52).
//!
//! The violations a case shows must equal, exactly, the schema defects its one entry lists. Each
//! defect is identified by its record (direction, kind and ordinal), schema path, instance path
//! and rule, so a second violation in another message of the same kind, or a changed violation,
//! fails on its own. An unlisted violation fails the case; a listed defect no longer shown fails
//! it as a fix to record. A wire or body record without its raw value, or a message the checker
//! could not judge, is a coverage gap and fails the case unless the case declares that gap.
//!
//! Intent is per record (addendum 2a): a deliberately invalid record exempts only its own
//! declared violation, which must still be shown; a synthetic-apparatus record's violations are
//! apparatus findings, kept apart from product defects.

use serde::Serialize;

use super::TapDirection;
use super::connection_ledger::UnansweredRequest;
use super::edge_conformance::Violation;
use super::expected_failures::{Deliverable, SchemaDefect};

/// What a test meant one record to be.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(tag = "intent", rename_all = "camelCase")]
pub enum RecordIntent {
    /// A message the product really sends or receives: its violations are product defects.
    #[default]
    Product,
    /// A bad message the test injects to prove rejection. Only this violation is exempt, and it
    /// must be shown.
    #[serde(rename_all = "camelCase")]
    DeliberateInvalid {
        keyword: &'static str,
        instance_path: &'static str,
        /// The rule's discriminator, as violations carry it (the missing property for
        /// `required`); empty when the keyword has none.
        detail: &'static str,
    },
    /// A stand-in from the test apparatus, such as a permissive schema: its violations are
    /// apparatus findings.
    SyntheticApparatus,
}

/// One observed schema defect, identified as the known-failure list identifies it.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefectIdentity {
    pub direction: String,
    pub kind: String,
    pub ordinal: u16,
    pub schema_path: String,
    pub instance_path: String,
    pub keyword: String,
    pub detail: String,
}

impl DefectIdentity {
    fn of_listed(defect: &SchemaDefect) -> Self {
        Self {
            direction: direction_label(defect.record.direction).to_owned(),
            kind: defect.record.kind.to_owned(),
            ordinal: defect.record.ordinal,
            schema_path: defect.schema_path.to_owned(),
            instance_path: defect.instance_path.to_owned(),
            keyword: defect.keyword.to_owned(),
            detail: defect.detail.to_owned(),
        }
    }
}

pub(super) const fn direction_label(direction: TapDirection) -> &'static str {
    match direction {
        TapDirection::Consumed => "consumed",
        TapDirection::Produced => "produced",
    }
}

/// A listed schema defect the case reproduced, with who fixes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedDefect {
    #[serde(flatten)]
    pub identity: DefectIdentity,
    pub deliverable: Deliverable,
}

/// What the checks of one case observed, before it meets the list.
#[derive(Debug, Default)]
pub(super) struct Observed {
    pub(super) product: Vec<DefectIdentity>,
    pub(super) apparatus: Vec<DefectIdentity>,
    pub(super) coverage_gaps: Vec<String>,
    /// Deliberately invalid records whose declared violation did not show.
    pub(super) injections_not_shown: Vec<String>,
}

impl Observed {
    /// Sorts one record's violations into product defects, its exempt injection and apparatus.
    pub(super) fn record_violations(
        &mut self,
        direction: TapDirection,
        kind: &str,
        ordinal: u16,
        intent: &RecordIntent,
        violations: &[Violation],
    ) {
        let identity = |violation: &Violation| DefectIdentity {
            direction: direction_label(direction).to_owned(),
            kind: kind.to_owned(),
            ordinal,
            schema_path: violation.schema_path.clone(),
            instance_path: violation.instance_path.clone(),
            keyword: violation.keyword.clone(),
            detail: violation.detail.clone(),
        };
        match intent {
            RecordIntent::Product => self.product.extend(violations.iter().map(identity)),
            RecordIntent::SyntheticApparatus => {
                self.apparatus.extend(violations.iter().map(identity));
            }
            RecordIntent::DeliberateInvalid {
                keyword,
                instance_path,
                detail,
            } => {
                let declared = |violation: &&Violation| {
                    violation.keyword == *keyword
                        && violation.instance_path == *instance_path
                        && violation.detail == *detail
                };
                if !violations.iter().any(|violation| declared(&violation)) {
                    self.injections_not_shown.push(format!(
                        "{} {kind} #{ordinal}: declared {keyword} {detail:?} at {instance_path:?}",
                        direction_label(direction)
                    ));
                }
                self.product.extend(
                    violations
                        .iter()
                        .filter(|violation| !declared(violation))
                        .map(identity),
                );
            }
        }
    }
}

/// The schema side of a finished case.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSettlement {
    /// Listed defects the case reproduced.
    pub expected: Vec<ListedDefect>,
    /// Shown, but not listed.
    pub unlisted: Vec<DefectIdentity>,
    /// Listed, but no longer shown: the deliverable fixed it, so its entry must go.
    pub fixed: Vec<ListedDefect>,
    /// Records the case could not check, and why.
    pub coverage_gaps: Vec<String>,
    /// Coverage gaps the case declares by name; these do not fail it.
    pub declared_gaps: Vec<String>,
    pub injections_not_shown: Vec<String>,
    /// Violations of apparatus records: findings, never product defects.
    pub apparatus: Vec<DefectIdentity>,
    /// Requests no reply answered in the case's capture.
    pub unanswered: Vec<UnansweredRequest>,
}

impl SchemaSettlement {
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.unlisted.is_empty()
            && self.fixed.is_empty()
            && self.coverage_gaps.is_empty()
            && self.injections_not_shown.is_empty()
    }
}

/// Compares what a case observed with the schema defects its entry lists.
pub(super) fn settle(
    mut observed: Observed,
    listed: &[SchemaDefect],
    declared_gaps: Vec<String>,
    unanswered: Vec<UnansweredRequest>,
) -> SchemaSettlement {
    observed.product.sort();
    observed.product.dedup();
    let listed: Vec<ListedDefect> = listed
        .iter()
        .map(|defect| ListedDefect {
            identity: DefectIdentity::of_listed(defect),
            deliverable: defect.deliverable,
        })
        .collect();
    let unlisted = observed
        .product
        .iter()
        .filter(|shown| !listed.iter().any(|entry| &entry.identity == *shown))
        .cloned()
        .collect();
    let (expected, fixed) = listed
        .into_iter()
        .partition(|entry| observed.product.contains(&entry.identity));
    SchemaSettlement {
        expected,
        unlisted,
        fixed,
        coverage_gaps: observed.coverage_gaps,
        declared_gaps,
        injections_not_shown: observed.injections_not_shown,
        apparatus: observed.apparatus,
        unanswered,
    }
}
