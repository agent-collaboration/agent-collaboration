//! The same bounded organization list is readable from either admitted machine.
use super::call;
use crate::{ConformanceResult, native_board_session::NativeBoardSession};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPageRequest, CatalogAutomation,
};
use std::collections::BTreeSet;

pub(super) async fn run(
    machine: &mut NativeBoardSession,
    target_service: &str,
) -> ConformanceResult<()> {
    let mut cursor = None;
    let mut cursors = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut replaced_wakes = 0;
    loop {
        let AutomationAnswer::List { automations, next } = call(
            machine,
            AutomationCall::List(AutomationPageRequest {
                cursor: cursor.clone(),
                limit: 1.try_into()?,
            }),
        )
        .await?
        else {
            return Err("organization list did not return its bounded page".into());
        };
        if automations.len() > 1 {
            return Err("organization list exceeded its record limit".into());
        }
        for automation in automations {
            let id = match automation {
                CatalogAutomation::Wake(snapshot) => {
                    if String::from(
                        snapshot
                            .definition
                            .message
                            .target
                            .endpoint
                            .service_id
                            .clone(),
                    ) == target_service
                        && String::from(snapshot.definition.message.target.session_id.clone())
                            == "wake-target"
                    {
                        if snapshot.state != collaboration_protocol::WakeState::Cancelled
                            || snapshot.first_firing.is_some()
                        {
                            return Err("global Wake list lost the cancelled central state".into());
                        }
                        replaced_wakes += 1;
                    }
                    snapshot.definition.wakeup_id.as_str().to_owned()
                }
                CatalogAutomation::Schedule(snapshot) => {
                    if matches!(&snapshot.definition.destination,collaboration_protocol::ExecutionDestination::OwnedThread {target,..} if String::from(target.endpoint.service_id.clone())==target_service)
                    {
                        return Err("a deleted schedule remained in the organization list".into());
                    }
                    snapshot.schedule_id.as_str().to_owned()
                }
            };
            if !ids.insert(id) {
                return Err("organization list repeated a definition".into());
            }
        }
        match next {
            None => break,
            Some(next) => {
                if !cursors.insert(next.clone()) {
                    return Err("organization cursor repeated".into());
                }
                cursor = Some(next);
            }
        }
    }
    if replaced_wakes != 3 {
        return Err(format!(
            "organization list retained {replaced_wakes} replacement Wakes, expected 3"
        )
        .into());
    }
    Ok(())
}
