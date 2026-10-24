//! Source and parsed graph share one 64 KiB source budget.
use super::super::support::TestResult;
use fleet_admission::ProcessDefinitionInput;
use message_board::*;

pub(super) fn maximum_definition() -> TestResult<ProcessDefinitionInput> {
    let graph = ProcessDefinitionSource {
        initial_status: StatusName::try_from("\"".repeat(MAX_NAME_BYTES))?,
        edges: vec![ProcessEdge {
            name: MoveName::try_from("\"".repeat(MAX_NAME_BYTES))?,
            from: StatusName::try_from("\"".repeat(MAX_NAME_BYTES))?,
            to: StatusName::try_from(format!("{}\\", "\"".repeat(MAX_NAME_BYTES - 1)))?,
            required_role: AuthorityRole::Lead,
            guards: (0..MAX_EDGE_GUARDS)
                .map(|index| {
                    Ok(GuardRequirement {
                        name: ResourceName::try_from(format!(
                            "{}{}",
                            "\"".repeat(MAX_NAME_BYTES - 3),
                            (0..3)
                                .map(|bit| if index & (1 << bit) != 0 { '\\' } else { '"' })
                                .collect::<String>()
                        ))?,
                        evaluator: Identity::Human {
                            human_id: HumanId::try_from("\"".repeat(if index < 7 {
                                4096
                            } else {
                                761
                            }))?,
                        },
                    })
                })
                .collect::<TestResult<Vec<_>>>()?,
        }],
    };
    let mut source = serde_json::to_string(&graph)?;
    let remaining = MAX_MESSAGE_TEXT_BYTES
        .checked_sub(source.len())
        .ok_or("maximum definition exceeded its joint source bound")?;
    source.push_str(&"\n".repeat(remaining));
    assert_eq!(source.len(), MAX_MESSAGE_TEXT_BYTES);
    assert_eq!(ProcessDefinitionSource::parse(&source)?, graph);
    Ok(ProcessDefinitionInput {
        reference: ProcessDefinitionRef {
            name: super::support::name()?,
            version: super::support::name()?,
            // Independent Python hashlib SHA-256 of this exact padded source.
            source_hash: CanonicalHash::try_from(
                "a6eedda8300e8a3bd63018396aa52ec3de23d266edb2cdb31e0e4bdd7fe05d9a".to_owned(),
            )?,
        },
        source,
    })
}
