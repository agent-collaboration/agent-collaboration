//! Durable domain deadlines determine which face owns a physical alarm firing.
use crate::*;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaceAlarm {
    pub face: CallerKind,
    pub at: RegistryTime,
}
pub fn read_object_alarm_plan(
    tx: &mut dyn SqlTx,
    address: &ObjectAddress,
) -> Result<Vec<FaceAlarm>, CentralError> {
    let mut alarms = Vec::new();
    let mut add = |face, at| {
        if let Some(at) = at {
            alarms.push(FaceAlarm { face, at });
        }
    };
    match address {
        ObjectAddress::Connection(_) => {
            if crate::registry_dispatch::has_table(tx, "_session_state")? {
                add(CallerKind::Session, crate::forward_rules::next_alarm(tx)?);
                add(
                    CallerKind::Delivery,
                    crate::session_notices::delivery_alarm(tx)?,
                );
            }
        }
        ObjectAddress::Registry => {
            let directory = crate::lease_publication::next_alarm(tx)?;
            let cleanup = crate::claim_cleanup::next_alarm(tx)?;
            add(
                CallerKind::Registry,
                match (directory, cleanup) {
                    (Some(left), Some(right)) => Some(RegistryTime(left.0.min(right.0))),
                    (left, right) => left.or(right),
                },
            );
        }
        ObjectAddress::Copy(_) => {
            let directory = crate::directory_producer::next_alarm(tx)?;
            let parts = if crate::registry_dispatch::has_table(tx, "_copy_staged_parts")? {
                crate::copy_receipts::next_part_alarm(tx)?
            } else {
                None
            };
            add(
                CallerKind::Copy,
                match (directory, parts) {
                    (Some(left), Some(right)) => Some(RegistryTime(left.0.min(right.0))),
                    (left, right) => left.or(right),
                },
            );
        }
        ObjectAddress::Fleet => add(
            CallerKind::Fleet,
            crate::fleet_notice_outbox::next_alarm(tx)?,
        ),
    }
    Ok(alarms)
}
