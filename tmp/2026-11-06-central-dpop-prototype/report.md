# Central DPoP prototype verdict

**Feasible on both hosts.** The same runtime-free Rust verifier passes all 43
fixed JOSE vectors with native ring signatures on Tokio and native WebCrypto
signatures in the compiled Rust Worker on local workerd. These include 7 allowed
cases and 36 precise refusals. No wasm p256 implementation is used.

**Local elapsed cost is about 0.149 ms per complete token-plus-proof check.**
This includes two fresh WebCrypto key imports, Ed25519 token verification,
ES256 proof verification, parsing, thumbprint/token hashing and canonical intent
comparison. **Cloudflare CPU/billing cost remains unmeasured:** local tail events
reported `cpuTime=0` and `wallTime=0` even for batches whose measured elapsed time
was 145–160 ms. Those counters cannot support a CPU cost estimate.

This is a draft experiment, not production admission or a shipped protocol.
No PR, deployment, production replacement, new table or replay storage was made.

## Target and evidence

Product source tested: `09a23bc78a55835704e386f66bd0ef3edcdd8128`, branch
`item2/b2-central-dpop-prototype`, based on
`8df6e7599ae555f463133bc889e2a2a8aac46b0b`.
Subsequent handback changes contain the generated architecture baseline, this
report, operation transcripts and receipts; the tested Rust/TS verifier and
fixture bytes remain unchanged.

All heavy runs happened on **Sunclaw, Apple M1 Pro, 10 CPUs, macOS 26.6.2,
arm64**. Tools: Rust 1.98.1, Node 26.11.1, JOSE 6.2.12, worker-build 0.8.7,
Miniflare 5.20261012.0-beta and workerd 1.20261012.1 (`workerd 2026-10-12`).
The Worker uses the root `worker-release` profile, with wasm-opt skipped as in
the repository build command. It is the actual compiled Rust Worker, not a JS
reimplementation of the verifier.

Receipts are under [sunclaw-09a23bc78](sunclaw-09a23bc78/):

- [Raw Worker observations](sunclaw-09a23bc78/workerd-measurement.json),
  [timing summary](sunclaw-09a23bc78/timing-summary.json),
  [artifact hashes and host details](sunclaw-09a23bc78/artifact-receipt.json).
- [Original command/exits](sunclaw-09a23bc78/steps.json),
  [completion command/exits](sunclaw-09a23bc78/completion-steps.json).
- [Native vectors](sunclaw-09a23bc78/native-vectors.log),
  [broader native run](sunclaw-09a23bc78/native-scope-suite.log),
  [verification package tests](sunclaw-09a23bc78/verification-tests.log).

Compiled bundle SHA-256:

| Artifact | SHA-256 |
| --- | --- |
| `build/index.js` | `fc19de10459ff2d029a80e0599b17251d25bcd74cf1b8cf26fbe3c7cc9ce638b` |
| `build/index_bg.wasm` | `17cc8cde25adf8641c93ef1618937f22f957729eedac8e982a98ee0f06869557` |
| `person-dpop.json` | `0998b8d8dc8fce2475ce6c1346e1cbd074a2654bc6fb9cc28540e125934ffc03` |

The bundle remains in the corresponding sunclaw worktree's
`crates/collaboration-central-cloudflare/build/` for inspection.

## What the verifier checks

[Core](../../crates/collaboration-central-core/src/person_dpop_prototype/proof_verification.rs)
owns parsing, allowed algorithms/key selection, claims and hashes.
[Tokio](../../crates/collaboration-central-tokio/src/person_dpop_crypto.rs)
injects ring; [Worker](../../crates/collaboration-central-cloudflare/src/person_dpop_crypto.rs)
injects `crypto.subtle` through the same `DpopCrypto` trait. Its futures need no
Tokio runtime or Send constraint on the Worker.

The person JWT uses the sign-in service's pinned EdDSA/Ed25519 algorithm. Its
trusted fixture JWKS is selected by `kid`; signature, `iss`, `aud`, closed profile,
identity, `cnf.jkt` and all three signed expiry bounds are checked. Effective
expiry is the minimum of native `exp * 1000`, actual session deadline and fixed
authorization snapshot deadline. Equality is expired; proof leeway never extends
these bounds.

The proof must have `typ=dpop+jwt`, `alg=ES256` and a public P-256 JWK. The core
checks its signature, RFC 7638 thumbprint against `cnf.jkt`, POST method, configured
canonical external `htu`, second-resolution `iat`, `ath=SHA256(raw token)`, and
`jti=request ID`. Request IDs are canonical UUIDv7; their millisecond timestamp
and the separate proof iat obey `N-65000 < T <= N+5000`; request time must also
be strictly above the supplied watermark. The clock is injected and sampled
after asynchronous crypto, not cached at ingress.

The intent specimen is a closed `renameWorkspace` body. Its signed `intent`
claim is SHA-256/base64url of the UTF-8 JSON tuple:

```text
["fleet-person-intent-prototype-v1", organization_id, person_id, operation_id,
 "renameWorkspace", workspace_id, display_name]
```

Every semantic specimen field is covered. Body key order/whitespace do not
change it; unknown fields are refused. Unicode, quotes and a newline are in the
shared vector. Refreshed tokens/proofs/request IDs leave stable operation intent
unchanged. This encoding is **prototype-only**, not adoption of the future Fleet
wire. The owner selected the signed claim as carrier; the 🦁 Lead explicitly
accepted a specimen format for this experiment.

Results are `Verified { request }` or `Refused { reason }`, with a closed bounded
reason enum. A verified result is not a grant to apply an operation.

## Checks and outcomes

Each command ran from the repository root on sunclaw. The complete arguments
and exit codes are retained in the receipt JSON above.

| Check | Result |
| --- | --- |
| `cargo nextest run --locked -p collaboration-central-tokio --test integration -E 'test(person_dpop::)'` | exit 0; 3 passed, 25 skipped; includes all 43 JOSE vectors |
| `node verification/tooling/measure-person-dpop.ts <output>` | exit 0; 43/43 matching Worker outcomes |
| Native core/Tokio `cargo clippy --all-targets -- -D warnings` | exit 0 |
| Worker `cargo clippy --target wasm32-unknown-unknown --features conformance -- -D warnings` | exit 0 |
| Worker release build with `--locked --features conformance` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| verification `check`, `lint`, `format:check` | each exit 0 |
| `pnpm --dir verification test` | exit 0; 1062 passed, 0 failed/skipped |
| `ARCHITECTURE_BLESS=1 cargo nextest run --locked -p workspace-architecture-check --run-ignored only` | exit 0; 1 passed, 22 skipped; shrink only |
| Native default-feature core/Tokio scope suite | exit 100; 291 passed, 2 failed, 0 skipped; held base failures below |

The vector test compares exact reasons, not just any rejection: wrong issuer
signing key, wrong proof signing key, a valid proof made by another key, changed
body/operation/target/intent, old/future iat, missing/wrong ath, Bearer downgrade,
wrong htu/method, UUID/window/watermark boundaries, invalid profile/audience/
issuer and all three independent expiry endpoints. Native extras prove expiry
is sampled after crypto completion and conflicting declared JWK usage is refused.

No pre-implementation behavioral red or mutation sensitivity run is claimed for
this new experimental verifier. Real signatures and the exact negative oracles
were exercised on both hosts. Native signing went through the repository runner;
sunclaw reported no Developer ID identity and ran linker-signed, as its documented
fallback permits. The Cargo logs also retain nonfatal workers-rs template-manifest
diagnostics; command success does not mean that text was absent.

## Local timing, not billed CPU

Workload fixed before measuring: 43 correctness requests, 100 warmup calls, then
10 serial batches of 1000 complete verification calls with the same known-valid
input. Every call imports both keys; there is no application key cache, JWKS fetch
or storage. Validation time is fixed; performance timing is real elapsed time.
Batch output retains its first result, checked as verified; this is a timing loop,
not a replay or intermittent-failure study.

| Observation | Local measurement |
| --- | --- |
| Worker elapsed per call, mean of batch averages | **0.1489 ms** |
| Worker elapsed per call, range of batch averages | **0.145–0.160 ms** |
| Worker elapsed per call, median batch average | 0.1475 ms |
| Client batch elapsed divided by 1000, mean | 0.150893 ms |
| First correctness request after runtime ready | Worker 5 ms; client 36.122 ms |
| Observed local tail events | 53; every one reported CPU 0 and wall 0; no exceptions |

These are batch averages, not per-request percentiles or HTTP latency estimates.
The first request includes first-use overhead and is reported separately. The
machine's load, CPU, local runtime, key reuse and absence of network/storage limit
transfer to a deployed Worker. The zero tail counters do not mean crypto is free.
Actual Cloudflare invocation CPU/billing requires a deployed measurement under
its real accounting; this task neither deployed nor established that figure.

## Held B2a failures and the baseline refresh

The broader native run failed only:

- `copy_receiver::capacity::summary_storage_full_is_preserved_as_copy_unavailable_after_real_rollback`
- `copy_receiver::retry::lost_directory_ack_reopens_with_same_call_and_never_overwrites_a_newer_summary`

Both returned `Copy apply did not answer: NotSent { reason: Unreachable {
last_seen: None } }`. A focused run at the prototype head reproduced both failures
(exit 100; 0/2 passed, 26 skipped). A focused run at original base `8df6e7599`
also reproduced both with the same error (exit 100; 0/2 passed, 23 skipped):
[base receipt](sunclaw-09a23bc78/base-copy-reopen.log).

The fixture constructs a new `InMemoryCopyFleetReferences` on each open. The
existing core deliberately refuses writers when that private reference is
missing. The 🦁 Lead identified this as B2a's held Copy-routing work pending B1
tables and directed **no fixture repair here**, in message
`01a1b3cc-42bf-7090-819d-f82e9510079f`. No Copy fixture, routing implementation,
assertion, retry policy or timeout was changed. The broader suite is not green.

The original architecture gate reported one inherited shrink, not growth.
Base/current cargo-modules graphs have **2676 identical existing use edges**;
the prototype adds 54 edges entirely in its new namespace. See the
[comparison](sunclaw-09a23bc78/base-graph-comparison.json) and the retained
gzip-compressed raw DOT graphs. Compression preserves the emitted bytes,
including the tool's trailing whitespace.
The repository-prescribed bless reduced the large core SCC from 378 to 366 edges,
retaining its 26 modules. Its other SCCs did not change. It also recorded two
already-existing acyclic packages with empty SCC lists. No check script changed.
The 🦁 Lead confirmed this refresh in message `01a1b3c6-a366-7172-8105-17364cf23efd`.

## Limits and handback

- JWKS, person claims and operation data are synthetic JOSE fixtures, not a real
  sign-in-service issuance/snapshot path. Key fetching, rotation and caching are
  not verified. Better Auth's actual JWKS shape was checked in its pinned source:
  `use` is optional; a supplied non-signature usage is refused.
- No authorization floors, permissions, original-factor freshness, durable
  watermark/used-request storage, stable-operation ledger mapping, or applying
  transaction was added. Same-proof repetitions can verify in this experiment;
  replay prevention and applying-time rechecks remain later work.
- The specimen has one POST operation and verbatim canonical configured htu
  comparison. It is not a general URI-normalization/RFC conformance claim.
- The Worker route and its fixture policy/clock inputs are conformance-only.
  No production request path uses this verifier yet.
- No independent paired Verifier was commissioned; the 🦁 Lead owns review of
  this report and receipts. The report is this contribution's checkpoint and
  there are no board posts, per the commission clarification
  `01a1b39b-bdac-7741-a8ab-2ab3bea61839`.

The two Copy failures stay held; the prototype's scoped correctness and local
elapsed measurement are complete. Production integration and CPU billing stay
unverified. The Worker bundle and task tools are retained for review. After all
task builds/tests ended and open-file checks found no consumers, the repository's
`cargo clean --workspace --profile dev` removed 122 local workspace files
(63.3 MiB) and 932 sunclaw workspace files (1.9 GiB). Dependency caches remain.
The Worker hashes were checked again and are unchanged. See
[cleanup receipt](cleanup-receipt.json). The three passive wait loops were also
stopped after SSH was observed returning zero even for a remote `exit 17`;
completion was established from the retained test results, not that SSH status.

References: [advisor r3 sections 4/8](/Users/shravansunder/code/projects/agent-vm.design-portal-remediation/tmp/2026-11-06-push-revision-replay/advisor-design-r3.md:75),
[owner intent choice 2a](/Users/shravansunder/code/projects/agent-vm.design-portal-remediation/tmp/2026-10-30-lead-pages/fleet-access-tokens.html:267),
[sign-in key policy](/Users/shravansunder/code/projects/switchboard.item2-b2-central-organizations/packages/sign-in-core/src/better-auth/native-jwt-options.ts:5),
and the pinned Better Auth JWKS/JOSE implementation;
[RFC 9449](https://www.rfc-editor.org/rfc/rfc9449.html);
[Cloudflare timers](https://developers.cloudflare.com/workers/runtime-apis/performance/)
and [CPU versus wall limits](https://developers.cloudflare.com/workers/platform/limits/).
DeepWiki supplied API pointers; primary code/docs and real host runs supplied the
claims. An initial web open failed with HTTP 404; the primary pages were then
retrieved successfully with Tavily.
