//! Central schedule configuration preserves the existing preparation rules.
use crate::automation_store_error::{AutomationStoreError, AutomationStoreResult};
use collaboration_protocol::{
    AutomationRequestConstraint, ExecutionDestination, ScheduleDefinition,
};

pub(crate) fn definition(definition: &ScheduleDefinition) -> AutomationStoreResult<()> {
    let valid_choice = |value: Option<&String>| {
        value.is_some_and(|value| {
            !value.trim().is_empty() && !value.chars().any(char::is_whitespace)
        })
    };
    if !valid_choice(definition.effort.as_ref()) {
        return Err(invalid(
            "effort",
            AutomationRequestConstraint::RequiredToken,
        ));
    }
    if is_fresh(&definition.destination) && !valid_choice(definition.model.as_ref()) {
        return Err(invalid("model", AutomationRequestConstraint::RequiredToken));
    }
    match &definition.destination {
        ExecutionDestination::Unprepared | ExecutionDestination::FreshEachRunUnprepared
            if definition.enabled =>
        {
            Err(invalid(
                "enabled",
                AutomationRequestConstraint::UnpreparedDestination,
            ))
        }
        ExecutionDestination::OwnedThread { cwd, .. }
        | ExecutionDestination::FreshEachRun { cwd, .. }
            // This is the receiving Router's Unix path, not central's filesystem.
            // Rust's wasm32 Path::is_absolute rejects rooted Unix paths.
            if !cwd.starts_with('/') =>
        {
            Err(invalid(
                "destination.cwd",
                AutomationRequestConstraint::AbsolutePathRequired,
            ))
        }
        _ => Ok(()),
    }
}

pub(crate) fn unchanged_mode(
    before: &ExecutionDestination,
    after: &ExecutionDestination,
) -> AutomationStoreResult<()> {
    if is_fresh(before) != is_fresh(after) {
        return Err(invalid(
            "destination",
            AutomationRequestConstraint::ExecutionModeFixed,
        ));
    }
    Ok(())
}
fn is_fresh(destination: &ExecutionDestination) -> bool {
    matches!(
        destination,
        ExecutionDestination::FreshEachRunUnprepared | ExecutionDestination::FreshEachRun { .. }
    )
}
fn invalid(field: &'static str, reason: AutomationRequestConstraint) -> AutomationStoreError {
    AutomationStoreError::InvalidRequest { field, reason }
}
