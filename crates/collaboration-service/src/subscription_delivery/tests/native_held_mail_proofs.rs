//! Real readiness triggers for native held custody, with only the external client scripted.
//!
//! Held mail is checked when the reader first sees it and again only after a signal that
//! its target may have loaded: a Codex thread load recorded in the lifecycle journal, a
//! provider supervisor load, or an observation retry. There is no presence timer.
