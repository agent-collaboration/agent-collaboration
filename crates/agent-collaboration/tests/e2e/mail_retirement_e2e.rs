//! Terminal non-project mail survives replay and restart without sender retirement.

#![deny(clippy::too_many_lines)]
#[path = "../mail_retirement/readback.rs"]
mod readback;
#[path = "../mail_retirement/wake_and_capacity.rs"]
mod wake_and_capacity;
