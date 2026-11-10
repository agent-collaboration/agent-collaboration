# Router clock

One injected object supplies UTC timestamps, elapsed deadlines and waiting. Host
composition creates it once and passes it to its owners. Consumers that only read
UTC milliseconds use the same object as `runtime_primitives::Clock`.

This leaf owns time below both collaboration-runtime and lifecycle-observation,
so lifecycle retention does not need an upward dependency on collaboration-runtime.
The upstream Clock contract stays unchanged.

`SystemRuntimeClock` owns native time reads and Tokio timers. Tests use
`ManualClock`, available only through a non-default `test-support` feature enabled
by dev-dependencies. Advancing it changes UTC and elapsed time together and wakes
its sleepers. This crate's tests run without a Tokio time driver. Inherited SDK and SQLx internal timers remain tracked as clock C; their existing fault mechanisms are listed in the B proof map.
