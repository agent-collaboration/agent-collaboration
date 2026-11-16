//! Router-owned extensions (Program Design "Who owns each contract"): checked against the code that
//! owns them today. The owner's Rust codec decodes the raw value and encodes it again; the value
//! conforms when the codec accepts it and its encoding equals what crossed. A codec that rejects
//! the value, or re-encodes it differently, is an `owned-disagreement` (a `routerCodec`
//! violation). For values TypeScript produces, the audit report also runs the owning Zod schema
//! over the same raw value.
//!
//! The codecs live in the crates that own them, so the case registers them: test support never
//! depends on the parts it tests.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ContractOwner, EdgeConformance, MissingContext, Support, Violation, compact};

/// A Router-owned contract carried on an outside edge.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RouterContract {
    /// `_session/steering` params served by N9 (`SteeringRequest`).
    SteeringParams,
    SteeringResult,
    QueueAddParams,
    QueueAddResult,
    QueueListParams,
    QueueListResult,
    QueueCancelParams,
    QueueCancelResult,
    /// `_session/state` notification params.
    StateParams,
    /// `initialize` result `_meta` (`InitializeProfileMetadata`).
    InitializeMeta,
    /// `_meta.codexRouter.modelReasoning` on Claude's settings (N12).
    ModelReasoningMeta,
    /// `_meta` on N9's `session/new`, `session/load` and `session/resume` results
    /// (`SessionResponseProfileMetadata`).
    SessionResultMeta,
    /// `_meta.codexRouter` on an `available_commands_update` (the skill list status the Router's
    /// Claude adapter writes and `claude_behavior.rs` reads).
    SkillListMeta,
    /// `_meta.codexRouter` on one advertised command of that update (its skill kind).
    SkillCommandMeta,
}

impl RouterContract {
    /// The contract a Router-served extension method's params or result carries.
    #[must_use]
    pub fn for_method(method: &str, result: bool) -> Option<Self> {
        Some(match (method, result) {
            ("_session/steering", false) => Self::SteeringParams,
            ("_session/steering", true) => Self::SteeringResult,
            ("_session/queue/add", false) => Self::QueueAddParams,
            ("_session/queue/add", true) => Self::QueueAddResult,
            ("_session/queue/list", false) => Self::QueueListParams,
            ("_session/queue/list", true) => Self::QueueListResult,
            ("_session/queue/cancel", false) => Self::QueueCancelParams,
            ("_session/queue/cancel", true) => Self::QueueCancelResult,
            ("_session/state", false) => Self::StateParams,
            _ => return None,
        })
    }
}

/// Decodes a raw value with the owning type and encodes it again.
pub type OwnedCodec = fn(&Value) -> Result<Value, String>;

/// For a contract whose owning decoder has no encoder (model metadata, addendum 2c): the owning
/// Zod schema's verdict on the raw value and the Rust decoder's typed outcome, as `(zod, rust)`,
/// each rendered as a comparable value: `{"accepted": ...}` (the retained values, read through
/// public getters), `{"rejected": ...}`, or, for the Rust side only, `{"ambiguous": ...}` when its
/// outcome cannot say whether it accepted the value. A plain pair, so an owning crate can provide
/// it without depending on test support. An `Err` is an owner check that could not run (its
/// environment failed): incomplete, never a Router violation.
///
/// The frame's metadata conforms only when both owners accept it alike (correction 4, V2):
/// - both accept the same value: conforms;
/// - both reject: a violation (`bothReject`), since agreement on rejection certifies nothing;
/// - they differ: a violation (`zodVsRust`);
/// - Rust is ambiguous: with Zod accepting, incomplete (`OwnerOutcomeUnavailable`); with Zod
///   rejecting, the Rust fallback agrees with rejection, so both reject.
pub type OwnedVerdicts = fn(&Value) -> Result<(Value, Value), String>;

#[derive(Clone, Copy, Debug)]
enum OwnedCheck {
    RoundTrip(OwnedCodec),
    Verdicts(OwnedVerdicts),
}

/// The owning checks a case registered, by contract.
#[derive(Clone, Debug, Default)]
pub struct OwnedCodecs(BTreeMap<RouterContract, OwnedCheck>);

impl OwnedCodecs {
    /// Holds `contract` to a codec that decodes and re-encodes.
    pub fn register(&mut self, contract: RouterContract, codec: OwnedCodec) {
        self.0.insert(contract, OwnedCheck::RoundTrip(codec));
    }

    /// Holds `contract` to the agreement of its Zod verdict and its Rust decoder's outcome.
    pub fn register_verdicts(&mut self, contract: RouterContract, verdicts: OwnedVerdicts) {
        self.0.insert(contract, OwnedCheck::Verdicts(verdicts));
    }

    /// The owning checks an owner registry lists by contract name (`RouterContract`'s camelCase
    /// form): `acp_provider_adapters::owned_contract_checks`, which every part test and the live
    /// checker use. The registry names contracts as text so the owning crate never depends on
    /// test support.
    ///
    /// # Errors
    /// When a listed name is not a Router contract.
    pub fn from_owner_registry(
        round_trips: Vec<(&'static str, OwnedCodec)>,
        verdicts: Vec<(&'static str, OwnedVerdicts)>,
    ) -> Result<Self, String> {
        let contract = |name: &str| {
            serde_json::from_value::<RouterContract>(Value::String(name.to_owned()))
                .map_err(|_| format!("the owner registry lists an unknown contract {name}"))
        };
        let mut codecs = Self::default();
        for (name, codec) in round_trips {
            codecs.register(contract(name)?, codec);
        }
        for (name, check) in verdicts {
            codecs.register_verdicts(contract(name)?, check);
        }
        Ok(codecs)
    }
}

/// Holds `raw` to the owning codec of `contract`.
pub(super) fn check(
    codecs: &OwnedCodecs,
    contract: RouterContract,
    raw: &Value,
) -> EdgeConformance {
    let owner = ContractOwner::RouterExtension;
    let Some(check) = codecs.0.get(&contract) else {
        return EdgeConformance::Incomplete {
            owner,
            missing: MissingContext::NoOwnerCodec { contract },
        };
    };
    let definition = format!("{contract:?}");
    let codec = match check {
        OwnedCheck::RoundTrip(codec) => *codec,
        OwnedCheck::Verdicts(verdicts) => {
            return verdicts_agree(contract, *verdicts, definition, raw);
        }
    };
    let violation = match codec(raw) {
        Ok(encoded) if &encoded == raw => {
            return EdgeConformance::Conforms {
                owner,
                definition,
                support: Support::NotApplicable,
            };
        }
        Ok(encoded) => Violation {
            schema_path: definition.clone(),
            instance_path: String::new(),
            keyword: "routerCodec".to_owned(),
            detail: String::new(),
            expected: format!(
                "the owning codec to re-encode the value unchanged, got {}",
                compact(&encoded)
            ),
            found: compact(raw),
        },
        Err(error) => Violation {
            schema_path: definition.clone(),
            instance_path: String::new(),
            keyword: "routerCodec".to_owned(),
            detail: String::new(),
            expected: format!("a value the owning codec accepts: {error}"),
            found: compact(raw),
        },
    };
    EdgeConformance::Violations {
        owner,
        definition,
        violations: vec![violation],
    }
}

/// Compares the Zod verdict with the Rust decoder's outcome on the same raw value.
fn verdicts_agree(
    contract: RouterContract,
    verdicts: OwnedVerdicts,
    definition: String,
    raw: &Value,
) -> EdgeConformance {
    let owner = ContractOwner::RouterExtension;
    let (zod, rust) = match verdicts(raw) {
        Ok(pair) => pair,
        Err(reason) => {
            return EdgeConformance::Incomplete {
                owner,
                missing: MissingContext::OwnerOutcomeUnavailable { contract, reason },
            };
        }
    };
    let rejected = |verdict: &Value| verdict.get("rejected").is_some();
    let accepted = |verdict: &Value| verdict.get("accepted").is_some();
    let ambiguous = rust.get("ambiguous").is_some();
    let (expected, detail) = if ambiguous && accepted(&zod) {
        return EdgeConformance::Incomplete {
            owner,
            missing: MissingContext::OwnerOutcomeUnavailable {
                contract,
                reason: format!(
                    "the Zod schema accepts the value, and the Rust outcome {} cannot say whether the decoder accepted it or fell back (model_reasoning_metadata.rs:97 folds every failure into Unknown, a D1 finding)",
                    compact(&rust)
                ),
            },
        };
    } else if rejected(&zod) && (rejected(&rust) || ambiguous) {
        (
            format!(
                "a value both owners accept; the Zod verdict {} and the Rust outcome {} both reject it",
                compact(&zod),
                compact(&rust)
            ),
            "bothReject".to_owned(),
        )
    } else if zod == rust && accepted(&zod) {
        return EdgeConformance::Conforms {
            owner,
            definition,
            support: Support::NotApplicable,
        };
    } else {
        (
            format!(
                "the Zod verdict {} to equal the Rust outcome {}",
                compact(&zod),
                compact(&rust)
            ),
            "zodVsRust".to_owned(),
        )
    };
    EdgeConformance::Violations {
        owner,
        violations: vec![Violation {
            schema_path: definition.clone(),
            instance_path: String::new(),
            keyword: "routerCodec".to_owned(),
            detail,
            expected,
            found: compact(raw),
        }],
        definition,
    }
}
