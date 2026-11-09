//! The gate: the inventory's candidate must hash to the digest the inventory
//! named, and main's decoders must accept every rebuilt object. The pass names
//! that digest and run time; migrate takes both and refuses any other candidate.

use crate::candidate_image::{Candidate, InventoryAnswer};
use crate::main_decoders::{Refusal, decode_copy, decode_fleet};
use crate::object_rebuild::rebuild;
use crate::table_coverage::{COPY, Coverage, FLEET, coverage};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Pass,
    Refused,
}

/// The gate's report: what call_migration's `--gate` reads.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateReport {
    pub outcome: Outcome,
    /// SHA-256 of the candidate text this gate decoded; absent when there was none.
    pub candidate_digest: Option<String>,
    pub run_time_millis: Option<i64>,
    pub refusals: Vec<Refusal>,
}

fn refused(
    digest: Option<String>,
    run_time: Option<i64>,
    object: &str,
    reason: String,
) -> GateReport {
    GateReport {
        outcome: Outcome::Refused,
        candidate_digest: digest,
        run_time_millis: run_time,
        refusals: vec![Refusal {
            object: object.into(),
            row: String::new(),
            reason,
        }],
    }
}

pub async fn run_gate(inventory_text: &str) -> GateReport {
    let inventory: InventoryAnswer = match serde_json::from_str(inventory_text) {
        Ok(inventory) => inventory,
        Err(error) => return refused(None, None, "inventory", error.to_string()),
    };
    let run_time = Some(inventory.run_time_millis);
    if inventory.kind != "inventory" || !inventory.stops.is_empty() {
        let reason = format!(
            "the inventory is {} with {} stops; migrate must not run",
            inventory.kind,
            inventory.stops.len()
        );
        return refused(None, run_time, "inventory", reason);
    }
    let (Some(text), Some(named)) = (inventory.candidate_text, inventory.candidate_digest) else {
        let reason = "the inventory carries no candidate; take it with the Mac-file facts".into();
        return refused(None, run_time, "inventory", reason);
    };
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    if digest != named {
        let reason = format!("the candidate text hashes to {digest}, not the {named} it names");
        return refused(Some(digest), run_time, "candidate", reason);
    }
    let candidate: Candidate = match serde_json::from_str(&text) {
        Ok(candidate) => candidate,
        Err(error) => return refused(Some(digest), run_time, "candidate", error.to_string()),
    };
    let mut refusals = covered("fleet", &candidate.fleet, FLEET);
    for (project, image) in &candidate.copies {
        refusals.extend(covered(&format!("copy {project}"), image, COPY));
    }
    match rebuild(&candidate.fleet).await {
        Ok(mut connection) => match decode_fleet(&candidate.fleet, &mut connection).await {
            Ok(found) => refusals.extend(found),
            Err(error) => refusals.push(storage_refusal("fleet", &error)),
        },
        Err(error) => refusals.push(storage_refusal("fleet", &error)),
    }
    for (project, image) in &candidate.copies {
        let object = format!("copy {project}");
        match rebuild(image).await {
            Ok(mut connection) => match decode_copy(project, &mut connection).await {
                Ok(found) => refusals.extend(found),
                Err(error) => refusals.push(storage_refusal(&object, &error)),
            },
            Err(error) => refusals.push(storage_refusal(&object, &error)),
        }
    }
    GateReport {
        outcome: if refusals.is_empty() {
            Outcome::Pass
        } else {
            Outcome::Refused
        },
        candidate_digest: Some(digest),
        run_time_millis: run_time,
        refusals,
    }
}

/// A table no coverage entry names, or a must-be-empty table with rows, is refused.
fn covered(
    object: &str,
    image: &crate::candidate_image::ObjectImage,
    map: &[(&str, Coverage)],
) -> Vec<Refusal> {
    image
        .tables
        .iter()
        .filter_map(|(table, rows)| {
            match coverage(map, table) {
                None => Some(format!("{table} has no gate coverage")),
                Some(Coverage::MustBeEmpty(what)) if !rows.rows.is_empty() => Some(format!(
                    "{table} holds {} {what} rows; main decodes them and the gate doesn't",
                    rows.rows.len()
                )),
                Some(_) => None,
            }
            .map(|reason| Refusal {
                object: object.into(),
                row: table.clone(),
                reason,
            })
        })
        .collect()
}

fn storage_refusal(object: &str, error: &dyn std::fmt::Display) -> Refusal {
    Refusal {
        object: object.into(),
        row: "rebuild".into(),
        reason: error.to_string(),
    }
}
