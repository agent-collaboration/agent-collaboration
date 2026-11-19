//! Verified seed and saved retirement floors exclude old custody without skipping new work.

// This setup spans several physical owners. Keep its state on the heap so
// callers do not inline it beside the native migration/parser call stack.
