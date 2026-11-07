# Central DPoP prototype checkpoint

Work in progress. No verification or performance verdict yet.

Commission: Lead message 01a1b39a-46f5-7fa3-a265-674ba5323b34, clarification
01a1b39b-bdac-7741-a8ab-2ab3bea61839. Branch `item2/b2-central-dpop-prototype`,
base `8df6e7599ae555f463133bc889e2a2a8aac46b0b`. Contribution only; the Lead
waived a paired Verifier and board posts, and named this report as checkpoint.
Project `01a14e9e-ba87-7605-bc7b-7e6d49d07e5f`, thread prefix `01a16fc6`.

## Model and authority

- Advisor design r3 sections 4 and 8 govern the signed expiry bounds and
  `N-65000 < T <= N+5000`, `T > watermark`, proof jti = transport request ID.
- The owner chose an `intent` claim. Its exact canonical encoding is not adopted;
  the Lead accepted this prototype's versioned tuple specimen in message
  01a1b39f-a059-7d13-a8e3-e56f1ed6fb70. This is not a public protocol change.
- Source `sign-in-core/src/better-auth/native-jwt-options.ts` pins EdDSA issuer
  tokens. The Lead accepted Ed25519 tokens plus ES256 proofs in that message.
- Core owns parsing, key selection, claim rules, hashes and intent. Native and
  Worker adapters own signature mechanics. Worker uses WebCrypto, not wasm p256.
- No new storage, used-request markers, production wiring, deployment or PR.
  The conformance-only probe measures full uncached imports and verification.

## Proof plan and limitations

Fixed JOSE vectors run through real native and workerd signature primitives with
an injected validation clock. Each negative asserts the precise refusal reason.
The three expiry negatives leave the other two bounds live. No pre-implementation
behavioral red exists: this is new throwaway prototype code; mutation sensitivity
will be recorded separately if run.

CPU metric availability is an open measurement question. Local `performance.now`
is elapsed time and will not be reported as CPU time or billed CPU. Local native
crypto and machine load are not deployed Cloudflare performance or billing proof.

## Current actions

- Read the named design and owner intent page, current core/host Access crypto,
  pinned sign-in algorithm, workers-rs crypto and JOSE verify/thumbprint sources.
- Added bounded core verifier, native adapter, conformance-only Worker adapter
  and probe, JOSE generator and native integration check. Checks pending.
- Sunclaw preflight: broken old cargo symlink, usable rustup proxy at
  `/opt/homebrew/opt/rustup/bin` per Lead; task-local tool install approved.

Next: run focused native and workerd proof from a pushed head on sunclaw, preserve
raw logs, report elapsed/available CPU metrics and exact unverified obligations.
