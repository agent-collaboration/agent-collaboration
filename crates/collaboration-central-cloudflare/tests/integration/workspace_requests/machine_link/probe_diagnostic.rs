//! Allowlisted handoff of one apparatus diagnostic after real fixture disposal.
use super::super::workerd_host::c2_controls::{AttachmentSnapshot, WitnessEvent, WitnessRecord};
use central_storage::SafeInteger;
use serde::{Deserialize, Serialize};

const INPUT_PREFIX: &str = "C2_ORDINARY_ACCEPT_PROBE ";
const RESULT_PREFIX: &str = "C2_ORDINARY_ACCEPT_PROBE_RESULT ";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ProbeStage {
    Eviction,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ProbeDisposition {
    ActiveReferences,
    UnclassifiedEvictionFailure,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct ProbeDiagnostic {
    stage: ProbeStage,
    disposition: ProbeDisposition,
    events: Vec<WitnessRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("ordinary accept probe evidence is unavailable or invalid")]
pub(super) struct DiagnosticRefusal;

/// Disabled mode and successful journeys do not inspect or publish child logs.
pub(super) fn collect(
    enabled: bool,
    journey_failed: bool,
    logs: &[String],
) -> Result<Option<ProbeDiagnostic>, DiagnosticRefusal> {
    if !enabled || !journey_failed {
        return Ok(None);
    }
    let mut anchored = logs
        .iter()
        .filter_map(|line| line.strip_prefix(INPUT_PREFIX));
    let encoded = anchored.next().ok_or(DiagnosticRefusal)?;
    if anchored.next().is_some() {
        return Err(DiagnosticRefusal);
    }
    let diagnostic: ProbeDiagnostic =
        serde_json::from_str(encoded).map_err(|_| DiagnosticRefusal)?;
    super::witness_assertions::valid_records(&diagnostic.events).map_err(|_| DiagnosticRefusal)?;
    for record in &diagnostic.events {
        if record.instance_id.get_variant() != uuid::Variant::RFC4122 {
            return Err(DiagnosticRefusal);
        }
        match &record.event {
            WitnessEvent::Upgrade { attachments, .. } => {
                for attachment in attachments {
                    validate_attachment(attachment)?;
                }
            }
            WitnessEvent::Handler { attachment, .. } => validate_attachment(attachment)?,
            WitnessEvent::AlarmSnapshot {
                earliest_due_ms,
                alarm_ms,
                ..
            } => {
                if earliest_due_ms.is_some_and(|time| time.get() < 0)
                    || alarm_ms.is_some_and(|time| time.get() < 0)
                {
                    return Err(DiagnosticRefusal);
                }
            }
            WitnessEvent::Constructor { .. }
            | WitnessEvent::EnrollmentRead { .. }
            | WitnessEvent::RestoreResult { .. } => {}
        }
    }
    Ok(Some(diagnostic))
}

fn validate_attachment(attachment: &AttachmentSnapshot) -> Result<(), DiagnosticRefusal> {
    let canonical_v7 = |value: &str| {
        uuid::Uuid::parse_str(value).is_ok_and(|id| {
            id.get_version_num() == 7
                && id.get_variant() == uuid::Variant::RFC4122
                && id.to_string() == value
        })
    };
    if attachment.machine_id.get_version_num() != 7
        || attachment.machine_id.get_variant() != uuid::Variant::RFC4122
        || [
            attachment.organization_id.as_str(),
            attachment.installation_id.as_str(),
            attachment.enrollment_id.as_str(),
            attachment.client_id.as_str(),
        ]
        .iter()
        .any(|value| !canonical_v7(value))
        || !(0..=SafeInteger::MAX.get()).contains(&attachment.effective_expires_at.0)
    {
        return Err(DiagnosticRefusal);
    }
    Ok(())
}

#[derive(Serialize)]
struct DisposedProbeDiagnostic {
    stage: ProbeStage,
    disposition: ProbeDisposition,
    events: Vec<WitnessRecord>,
    disposed: bool,
}

/// The caller supplies disposal only after awaited host and key-server cleanup.
pub(super) fn result_marker(
    diagnostic: Result<Option<ProbeDiagnostic>, DiagnosticRefusal>,
    disposed: bool,
) -> Result<Option<String>, DiagnosticRefusal> {
    if !disposed {
        return Err(DiagnosticRefusal);
    }
    let Some(diagnostic) = diagnostic? else {
        return Ok(None);
    };
    let encoded = serde_json::to_string(&DisposedProbeDiagnostic {
        stage: diagnostic.stage,
        disposition: diagnostic.disposition,
        events: diagnostic.events,
        disposed: true,
    })
    .map_err(|_| DiagnosticRefusal)?;
    Ok(Some(format!("{RESULT_PREFIX}{encoded}")))
}

#[cfg(test)]
#[path = "probe_diagnostic_test.rs"]
mod tests;
