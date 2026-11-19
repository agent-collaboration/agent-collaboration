//! Held native mail survives each failure on the way to its next check.
//!
//! A held item is marked checked only once its presence check, its journal position and
//! any release all succeed. A failure fails the pass, and its backoff checks again; a
//! journal position that cannot be read from owes a check that records a fresh one.
