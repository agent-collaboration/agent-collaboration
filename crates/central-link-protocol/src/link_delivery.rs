//! Central delivering a direct message to the recipient's Router, and the
//! recipient's answer (2c, design 2026-11-17). A push is safe to repeat: the
//! recipient's inbox drops a message id it already has, and the Ack names the
//! message, so no per-send id is needed. How a message ended reaches its sender
//! as a stored message (MessageBody::DeliveryOutcome), not as a frame.
pub use workspace_model::central_mail::{
    DeliveryAnswer, MessageCancellation, MessageOutcome, MessageRefusal,
};
