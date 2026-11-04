# Opaque workerd timer handles

The Cloudflare `PlatformIo::reply_deadline` adapter uses an opaque JavaScript
timer handle instead of `worker::Delay`. Both the production object and the
bootstrap conformance object use the same adapter. Dropping the future calls
`clearTimeout` before releasing its retained Rust callback.

At workers-rs `b171cb4bdd6fa943bb81f429df0a6b0df37e14b3`,
`worker/src/delay.rs:53–59` calls the web-sys `setTimeout` binding returning
`i32`. Under the declared workerd compatibility date `2026-08-21`, the actual
timer handle is an object. The wasm-bindgen return conversion throws
`expected a number argument, found object`, and `Delay::poll` panics. No timer
duration, compatibility date, dependency pin, public API or stored data changes
to accommodate this defect.

The first real `fleet-mail-expiry` run reproduced this through the Fleet alarm's
Machine status dispatch. Local evidence is
`tmp/d1-central-implementation/c4-native-expiry-workerd-runtime.log:20–77`;
the harness stopped and reaped its child before returning failure. The adapter
keeps the platform value unchanged, passing it back only to `clearTimeout`.

To recreate the regression, build the Cloudflare conformance bundle and the
`collaboration-central-tokio` example `session_scope_workerd`, then run its
`fleet-mail-expiry` scenario with Node, the pinned Miniflare entrypoint, fresh
isolated persistence and a free loopback port. The scenario checks the exact
unsolicited Expired revision before recipient reconnect, then withholds a real
sentinel Push answer and observes that its exact waiter disappears through the
actual deadline. Earlier Machine status dispatch cancels its deadline, so the
same run also crosses a cancelled timer's former due time without invoking a
released Rust callback. Native owning-store reopen cases separately preserve
the expiry/recovery contract.

Remove this workaround when the adopted workers-rs `Delay` handles the deployed
workerd timer representation and the same real workerd expiry/deadline scenario
passes with both `PlatformIo` implementations switched back to that upstream
future. A native unit test alone cannot establish that removal condition.

Reference: [Cloudflare timer documentation](https://developers.cloudflare.com/workers/runtime-apis/nodejs/timers/).
