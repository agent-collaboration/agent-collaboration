//! The D4 seam controls on Tokio. Each is served from the slot and never
//! enters the object's event pipeline (S3).

use super::event_seams::SEAM_WAIT_BOUND;
use super::{NativeObjects, ObjectSlot, TokioObjectIo};
use collaboration_central_conformance::{
    ObjectFault, ObjectSeamControl, ObjectSeed, RuntimeControl, RuntimeControlAnswer,
    RuntimeControlFailure,
};
use collaboration_central_core::ObjectAlarm;
use runtime_primitives::WakeFailure;
use std::sync::Arc;

impl TokioObjectIo {
    /// The injected clock moved: end deadlines it passed and re-derive the
    /// alarm's firing instant. The stored alarm is never written.
    pub(super) async fn clock_changed(&self) -> Result<(), WakeFailure> {
        self.clock_moved.send_replace(());
        self.wake.clock_changed().await
    }
}

impl NativeObjects {
    pub(super) async fn seam_control(
        self: &Arc<Self>,
        slot: &ObjectSlot,
        control: ObjectSeamControl,
    ) -> Result<RuntimeControlAnswer, RuntimeControlFailure> {
        let seams = &slot.io.seams;
        Ok(match control {
            ObjectSeamControl::ReadAlarm => RuntimeControlAnswer::Alarm {
                stored: slot
                    .io
                    .wake
                    .stored()
                    .await
                    .map_err(|_| RuntimeControlFailure::Poisoned)?
                    .map(runtime_primitives::SyncTime::unix_millis),
            },
            ObjectSeamControl::AlarmDelivery { enabled } => {
                seams.set_alarm_delivery(enabled);
                RuntimeControlAnswer::Done
            }
            ObjectSeamControl::DuplicateAlarm => {
                if slot.alarm_task.get().is_none() {
                    return Err(RuntimeControlFailure::NotArmed);
                }
                tokio::time::timeout(SEAM_WAIT_BOUND, seams.request_duplicate_alarm())
                    .await
                    .map_err(|_| RuntimeControlFailure::WaitBoundExceeded)?
                    .map_err(|_| RuntimeControlFailure::NotArmed)?;
                RuntimeControlAnswer::Done
            }
            ObjectSeamControl::SettleClock { now } => {
                let before = seams.alarm_events();
                slot.io
                    .faults
                    .control(RuntimeControl::SetTime { now })
                    .await?;
                slot.io
                    .clock_changed()
                    .await
                    .map_err(|_| RuntimeControlFailure::Poisoned)?;
                // With no alarm loop there is nothing to drain.
                if slot.alarm_task.get().is_some() {
                    let epoch = slot.io.wake.clock_epoch();
                    tokio::time::timeout(SEAM_WAIT_BOUND, slot.io.wake.settled_through(epoch))
                        .await
                        .map_err(|_| RuntimeControlFailure::WaitBoundExceeded)?;
                }
                RuntimeControlAnswer::ClockSettled {
                    alarm_events: seams.alarm_events().saturating_sub(before),
                }
            }
            ObjectSeamControl::ReadEvents { after } => seams.read_events(after),
            ObjectSeamControl::RebuildAtQuiescence { enabled } => {
                seams.set_rebuild_at_quiescence(enabled);
                RuntimeControlAnswer::Done
            }
            ObjectSeamControl::ReadPending => seams.pending(
                slot.kernel
                    .get()
                    .map_or(0, super::object_slot::KernelCell::active),
            ),
            ObjectSeamControl::Barrier { point, unit_kind } => RuntimeControlAnswer::BarrierArmed {
                barrier_id: seams.arm_barrier(point, unit_kind),
            },
            ObjectSeamControl::AwaitBarrier { barrier_id } => RuntimeControlAnswer::Barrier {
                reached: true,
                event_id: seams.await_barrier(barrier_id).await?,
            },
            ObjectSeamControl::ReleaseBarrier { barrier_id } => {
                seams.release_barrier(barrier_id)?;
                RuntimeControlAnswer::Done
            }
            ObjectSeamControl::Seed {
                seed: ObjectSeed::StaleAlarm { at },
            } => {
                slot.io
                    .wake
                    .seed_stale(runtime_primitives::SyncTime::from_unix_millis(at))
                    .await
                    .map_err(|_| RuntimeControlFailure::Poisoned)?;
                RuntimeControlAnswer::Done
            }
            ObjectSeamControl::ArmFault {
                fault: ObjectFault::ExactAlarmWrite { on },
                count,
            } => RuntimeControlAnswer::FaultArmed {
                fault_id: slot.io.wake.arm_exact_write_fault(on, count),
            },
            ObjectSeamControl::ReadFault { fault_id } => {
                let (armed, reached, consumed) = slot
                    .io
                    .wake
                    .exact_write_fault(fault_id)
                    .ok_or(RuntimeControlFailure::NotArmed)?;
                RuntimeControlAnswer::Fault {
                    armed,
                    reached,
                    consumed,
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "seam_controls_test.rs"]
mod tests;
