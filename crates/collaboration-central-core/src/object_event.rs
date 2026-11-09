//! What one event into an object is, and what the shell does when it ends.
//!
//! A leaf contract: the kernel, both runtimes and both shells name these types,
//! and nothing here depends on any of them.

use durable_steps::CallFailure;

/// One delivery into an object.
#[derive(Debug)]
pub enum ObjectEvent {
    SocketOpened {
        binding: fleet_session::ConnectionBinding,
    },
    Frame {
        binding: fleet_session::ConnectionBinding,
        frame: Vec<u8>,
    },
    SocketClosed {
        binding: fleet_session::ConnectionBinding,
    },
    /// A call from another object or an admin route. `storage_bytes` is the
    /// shell's storage measurement, supplied only for a Copy Health read.
    Call {
        body: Vec<u8>,
        storage_bytes: Option<u64>,
    },
    Alarm,
}

impl ObjectEvent {
    pub(crate) fn runs_due_batch(&self) -> bool {
        // A call returns its answer when its event ends, so a batch would delay it.
        matches!(self, Self::Frame { .. } | Self::Alarm)
    }
}

/// What the shell does when the event ends.
#[derive(Debug)]
pub enum EventReply {
    /// Socket events: any answers were already sent.
    None,
    /// A socket event failed; the shell closes that connection, as before the kernel.
    SocketFailed,
    CallAnswer(Vec<u8>),
    CallFailure(CallFailure),
    AlarmDone,
    /// The only failure that propagates: the platform redelivers the alarm.
    AlarmRetry,
}
