//! Native startup proof fixtures and actual completion observations.

// Retain each real native reader until another acquisition actually times out.
// The caller resets timeout evidence afterward so only its own operation can satisfy it.
