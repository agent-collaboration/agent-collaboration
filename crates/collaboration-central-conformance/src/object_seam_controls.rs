//! The D4 seam controls (S2, S4, S7) and every seam control's answer.
//!
//! A shell serves these before and outside the object's event pipeline: a seam
//! control never opens, repairs, runs due work, recomputes, writes the alarm or
//! touches the batch claim.

use collaboration_central_core::RegistryTime;
use collaboration_central_core::event_trace::{BarrierPoint, EventRecord};
use durable_steps::Uuid;
use serde::{Deserialize, Serialize};

/// The seam half of `RuntimeControl`, split off by `RuntimeControl::into_seam`.
#[derive(Clone, Debug)]
pub enum ObjectSeamControl {
    /// The alarm the platform will deliver next. Both shells.
    ReadAlarm,
    /// Off: the shell drops Alarm events for the object (frames and calls still
    /// run) and emits one `centralAlarmDropped` line per drop. Both shells.
    AlarmDelivery { enabled: bool },
    /// Deliver one extra Alarm event now and answer when it has completed. Tokio.
    DuplicateAlarm,
    /// Move the injected clock like `setTime`, then answer once the alarm loop
    /// has settled on the move: every alarm event it caused has completed and
    /// its record is in the ring. Tokio.
    SettleClock { now: RegistryTime },
    /// Records after the cursor `after` from the object's ring. Tokio.
    ReadEvents { after: u64 },
    /// While on, an event or call entering an object with nothing active
    /// rebuilds the kernel and its handler caches from storage. Tokio.
    RebuildAtQuiescence { enabled: bool },
    /// Work the object's events created that is still alive. Tokio.
    ReadPending,
    /// Arm one latch that holds the first event reaching `point` (and
    /// `unit_kind`, when given) that no other latch has captured. Tokio.
    Barrier {
        point: BarrierPoint,
        unit_kind: Option<String>,
    },
    /// Answer once the latch holds its event (bounded wait). Tokio.
    AwaitBarrier { barrier_id: Uuid },
    /// Release the latch's event; the latch is spent. Tokio.
    ReleaseBarrier { barrier_id: Uuid },
    /// Put the object into a state a journey needs, through its own writer.
    Seed { seed: ObjectSeed },
    /// Make the next `count` matching operations fail once each. Tokio.
    ArmFault { fault: ObjectFault, count: u32 },
    /// How an armed fault has been used. Tokio.
    ReadFault { fault_id: Uuid },
}

/// S5 seeds. Slice 3 offers the stale alarm; the others arrive with their slices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ObjectSeed {
    /// Store the alarm at `at` (Unix ms) without arming its delivery: on Tokio
    /// it never fires until a recompute replaces it (the Nov 5 shape). On
    /// workerd the platform alarm is written as is and delivered by its clock.
    StaleAlarm { at: i64 },
}

/// S5 faults. Slice 3 offers the exact alarm write; the others arrive with their slices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ObjectFault {
    /// The exact alarm write (the recompute's or the repair's) made by an event
    /// of this kind fails.
    ExactAlarmWrite { on: FaultedEvent },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FaultedEvent {
    Frame,
    Call,
    Alarm,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeControlAnswer {
    /// Unix milliseconds, or null when no alarm is stored.
    Alarm {
        stored: Option<i64>,
    },
    /// `first_retained` is the oldest cursor still held (the next cursor when
    /// the ring is empty); a page whose `after` is below `first_retained - 1`
    /// is missing records. `through` is the newest cursor (0 before any).
    Events {
        records: Vec<CursoredEvent>,
        first_retained: u64,
        through: u64,
    },
    Pending {
        /// Events and calls still running in the object.
        tasks: u64,
        /// Live event deadlines.
        deadlines: u64,
        /// Object calls still waiting for their answer.
        waiters: u64,
    },
    BarrierArmed {
        barrier_id: Uuid,
    },
    Barrier {
        reached: bool,
        /// The held event's S1 `eventId`.
        event_id: Uuid,
    },
    ClockSettled {
        /// Alarm events (delivered or dropped) the move caused.
        alarm_events: u64,
    },
    FaultArmed {
        fault_id: Uuid,
    },
    Fault {
        armed: u32,
        /// Matching operations that met the fault while it was armed.
        reached: u32,
        /// Of those, the ones it made fail (one each, up to `armed`).
        consumed: u32,
    },
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CursoredEvent {
    pub cursor: u64,
    pub record: EventRecord,
}
