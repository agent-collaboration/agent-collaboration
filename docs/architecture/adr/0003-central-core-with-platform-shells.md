# ADR 0003: Central's rules live in a platform-free core

**Status:** accepted, 2026-10-11. **Decided by:** the owner (2026-10-11). **Design:** program design 3, C20 and §2a.

## Context
Central runs on Cloudflare (U32). Its rules must not be tied to Cloudflare, and the whole system must run on one machine during testing.

## Decision
- **The core crate.** `collaboration-central-core` holds every rule, compiles for native and wasm32, and depends on no platform crate. The rules cover piece apply, the fence, lease renewal and expiry, transfer steps, fleet admission and session dispatch.
- **Storage** is the only injected interface: `SqlStore`, with a synchronous transaction closure over statements the core builds.
- **Everything else is returned or passed in.** Every other effect (replies, notices, calls between objects) is returned as data for the platform shell to carry out after the commit. Time and IDs are plain arguments.
- **Two shells implement `SqlStore` and carry out effects:**
  - `collaboration-central-cloudflare`, the production Worker;
  - `collaboration-central-local`, a native axum server with one actor per object.
  - A conformance suite runs the same scenarios on both.
- **Standards:** the same as the rest of the workspace. That means `forbid(unsafe_code)`, no unwrap, expect or panic outside tests, and a CI check that bans platform crates from the core.

## Consequences
- The core is unit-testable without IO, and the system runs locally with no Cloudflare account.
- Each shell needs a small loop that carries out effects.
- A takeover is an explicit, stored step machine (Fencing → Granted).

## Revisit when
A rule turns out to need platform behaviour the interface can't express.
