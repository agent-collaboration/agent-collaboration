//! Deliver a real alarm event, through the object kernel, to a fixture's runtime.
use collaboration_central_core::{EventReply, KernelFace, ObjectEvent, ObjectKernel};

/// The platform would redeliver this alarm: the kernel's recompute failed.
#[derive(Debug)]
pub struct AlarmRetried;

impl std::fmt::Display for AlarmRetried {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("alarm event asked the platform to retry")
    }
}

impl std::error::Error for AlarmRetried {}

/// Each call is a fresh kernel, like an alarm delivered to a newly built object.
pub async fn deliver_alarm<Face: KernelFace>(face: Face) -> Result<(), AlarmRetried> {
    match ObjectKernel::new(face).on_event(ObjectEvent::Alarm).await {
        EventReply::AlarmRetry => Err(AlarmRetried),
        _ => Ok(()),
    }
}
