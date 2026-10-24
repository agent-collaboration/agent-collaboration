//! A registration gate preserves the stable refusal kind and readable detail.
use durable_steps::CallFailure;
use fleet_sync_protocol::{GateReason, GateReasonCode};
use message_board::Refusal;

pub(crate) fn render(reason: &Refusal) -> Result<GateReason, CallFailure> {
    let structured = serde_json::to_value(reason).map_err(|_| CallFailure::AnswerLost)?;
    let kind = structured
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or(CallFailure::AnswerLost)?;
    let detail = match reason {
        Refusal::Archived => "The workspace or board is archived.".to_owned(),
        Refusal::NotFound(record) => format!("Record {} was not found.", record.id.as_str()),
        Refusal::AlreadyExists(record) => format!("Record {} already exists.", record.id.as_str()),
        Refusal::NameConflict { name, existing } => format!(
            "The name \"{}\" is already used by record {}.",
            name.as_str(),
            existing.id.as_str()
        ),
        Refusal::InvalidShape { field } => format!("Invalid field: {field}."),
        Refusal::Move(_)
        | Refusal::Workstream(_)
        | Refusal::Task(_)
        | Refusal::Trail(_)
        | Refusal::Role(_)
        | Refusal::Thread(_)
        | Refusal::StaleRevision { .. }
        | Refusal::TopLevelPostCooldown { .. }
        | Refusal::Closed { .. }
        | Refusal::DefinitionConflict
        | Refusal::ProjectRejected { .. } => {
            serde_json::to_string(reason).map_err(|_| CallFailure::AnswerLost)?
        }
    };
    Ok(GateReason {
        code: GateReasonCode::new(kind).map_err(|_| CallFailure::AnswerLost)?,
        detail,
    })
}

#[cfg(test)]
#[allow(
    clippy::panic_in_result_fn,
    reason = "Literal contract assertions fail their test."
)]
mod tests {
    use super::*;
    #[test]
    fn creation_refusals_keep_literal_text_and_stable_wire_codes()
    -> Result<(), Box<dyn std::error::Error>> {
        let record: message_board::RecordRef =
            message_board::ProjectId::try_from("019f0000-0000-7000-8000-000000000469".to_owned())?
                .into();
        for (reason, code, detail) in [
            (
                Refusal::Archived,
                "archived",
                "The workspace or board is archived.",
            ),
            (
                Refusal::NotFound(record.clone()),
                "notFound",
                "Record 019f0000-0000-7000-8000-000000000469 was not found.",
            ),
            (
                Refusal::AlreadyExists(record.clone()),
                "alreadyExists",
                "Record 019f0000-0000-7000-8000-000000000469 already exists.",
            ),
            (
                Refusal::NameConflict {
                    name: "Taken title".to_owned().try_into()?,
                    existing: record,
                },
                "nameConflict",
                "The name \"Taken title\" is already used by record 019f0000-0000-7000-8000-000000000469.",
            ),
            (
                Refusal::InvalidShape {
                    field: "title".to_owned(),
                },
                "invalidShape",
                "Invalid field: title.",
            ),
        ] {
            let actual = render(&reason)?;
            assert_eq!(actual.code.as_str(), code);
            assert_eq!(actual.detail, detail);
        }
        Ok(())
    }
    #[test]
    fn other_typed_refusals_preserve_explicit_compact_structure()
    -> Result<(), Box<dyn std::error::Error>> {
        let actual = render(&Refusal::DefinitionConflict)?;
        assert_eq!(actual.code.as_str(), "definitionConflict");
        assert_eq!(actual.detail, r#"{"kind":"definitionConflict"}"#);
        Ok(())
    }
}
