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
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CursoredEvent {
    pub cursor: u64,
    pub record: EventRecord,
}
