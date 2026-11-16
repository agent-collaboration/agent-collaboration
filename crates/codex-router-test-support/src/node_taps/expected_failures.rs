//! The strict expected-failure list for node taps.
//!
//! Each entry is a tap case whose catalog row describes behavior a later
//! deliverable brings, or a current defect. The case must keep failing until
//! that deliverable lands; when it starts to pass, the tap test fails and the
//! deliverable removes its entry here. Entries live in one file per node.

mod n1;
mod n10;
mod n11;
mod n12;
mod n3;
mod n4;
mod n5;
mod n6;
mod n7;
mod n8;
mod n9;

use serde::Serialize;

use super::TapDirection;

/// Who fixes an entry (Project 2's re-cut): D1 the edges, D2 the inside, the later Codex CLI
/// project, or a party outside this repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Deliverable {
    D1,
    D2,
    CliProject,
    External(&'static str),
}

/// One case that must fail until its deliverables land. Case ids are unique: one entry lists
/// every defect the case is allowed, and the case settles only when the defects it shows are
/// exactly these (R36).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedFailure {
    /// `<node>/<case>`, as [`super::TapCase::case_id`] renders it.
    pub case_id: &'static str,
    /// The node's output or state differs from its catalog row, as the case's declared current
    /// defect.
    pub behavior: Option<BehaviorDefect>,
    /// Each violation one recorded message shows against its true definition.
    pub schema: &'static [SchemaDefect],
}

/// A part-behavior defect, in the Program Design's words.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BehaviorDefect {
    pub deliverable: Deliverable,
    pub defect: &'static str,
}

/// One violation of one recorded message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaDefect {
    pub deliverable: Deliverable,
    pub record: RecordKey,
    pub schema_path: &'static str,
    pub instance_path: &'static str,
    /// The rule broken: the JSON Schema keyword, or `method`, `routerCodec`, `rmcpDecode`,
    /// `codexParts`.
    pub keyword: &'static str,
    /// The rule's own discriminator (the missing property for `required`), empty when none.
    pub detail: &'static str,
}

/// Which recorded message: the nth (from 0) record of that kind and direction in the case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordKey {
    pub direction: TapDirection,
    pub kind: &'static str,
    pub ordinal: u16,
}

/// Every tap case that currently fails on purpose, grouped by node.
pub const EXPECTED_FAILURES: &[&[ExpectedFailure]] = &[
    n1::ENTRIES,
    n3::ENTRIES,
    n4::ENTRIES,
    n5::ENTRIES,
    n6::ENTRIES,
    n7::ENTRIES,
    n8::ENTRIES,
    n9::ENTRIES,
    n10::ENTRIES,
    n11::ENTRIES,
    n12::ENTRIES,
];
