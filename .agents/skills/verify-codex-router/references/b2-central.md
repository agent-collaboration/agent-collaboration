# B2 central: organization link and person admin

This reference owns lasting B2a–B2d journeys on local Tokio and real workerd.
Given a declared source/build and an authorized isolated execution allocation,
return the real commands, deciding outcomes, owning readbacks, artifact hashes,
source/configuration guards and cleanup evidence. Finish only when each assigned
row has an observation or a named gap; acceptance belongs to the 🦁 Lead.

## Target and prerequisites

The adopted B2 central reference is `e847a8886cf49883dde5809e929cc9664bb758ae`.
The accepted outcome is organization-owned provisioning and person admission
through the private link, with the refusal/storage oracles below. The permanent
entries below exercise that contract; later targets must declare their complete
SHA and changed expectations before execution.
Do not substitute this old SHA or its artifacts for the newly declared target.

Run from the declared repository root, on one isolated real-Git clone with its
normal `target/`. Obtain the build slot and keep other writers out until source
guards settle. Allocate a new owner-private evidence directory outside `target/`,
a fresh persistence directory for each journey, and a free loopback port excluding
protected ports. Record the initial clean status, HEAD and a sorted manifest of
every tracked file's SHA256 and mode; compare after each build/runtime boundary.
Never repair or restore a changed source tree to manufacture a passing guard.

The qualified inputs are Rust **1.98.1** with rustfmt, Clippy, rust-src and the
Wasm target; Node **26.11.1**; pnpm **12.14.0**; worker-build **0.8.7**;
Miniflare **5.20261012.0-beta** / workerd **1.20261012.1**; cargo-modules **0.28.0**.
Read `rust-toolchain.toml`, `verification/package.json`,
`verification/pnpm-lock.yaml` and `scripts/tooling/tool-versions.tsv` at the
declared target. Record their hashes and actual tool versions. The accepted
collection used nextest **0.9.151**; the repository tool declaration at that
revision says **0.9.149**. Preserve that distinction and have the allocation name
its nextest version; do not silently rewrite the repository pin.

On Sunclaw, MUST load [B2 Sunclaw setup](b2-sunclaw.md) and return qualified
tools, signing and source guards before installing dependencies or driving a
journey. Run commands separately, retaining each exit before the next command;
stop on a nonzero exit. Install dependencies inside the authorized boundary:

```sh
mise exec node@26.11.1 -- pnpm --dir verification install --frozen-lockfile
```

Use that clone's `verification/node_modules/miniflare/dist/src/index.js` and
record its package/lock binding.
No real sign-in credentials, production Router, Codex profile or hosted endpoint
is needed. Fixture JWT/DPoP verification remains real; issuer behavior is separate.

## Organization-link journey — lasting

The entry is the permanent Rust example
`crates/collaboration-central-tokio/examples/organization_link_workerd.rs`.
It starts fixture key servers and the existing
`scripts/tooling/run-central-link-workerd.ts` harness through
`organization_link_workerd/link_client.rs`. The fixture caller's `CENTRAL`
binding reaches central-link, whose `FLEET` binding reaches public central's
existing FleetObject; central-link owns no second Fleet store. Owner-only public
observer reads return that same organization's stamp, floors, workspaces,
records, request markers and decisions.

1. Build the faults cohort and guard it, then freeze both files outside `build/`:

   ```sh
   worker-build crates/collaboration-central-cloudflare --dev --no-opt -- --locked --features conformance-faults
   node verification/tooling/check-central-worker-bundle.ts faults
   FROZEN_FAULTS="$EVIDENCE/frozen-faults"
   mkdir "$FROZEN_FAULTS"
   cp crates/collaboration-central-cloudflare/build/index.js crates/collaboration-central-cloudflare/build/index_bg.wasm "$FROZEN_FAULTS/"
   ```

   Record SHA256s of `index.js` and `index_bg.wasm`; the guard must exit zero and report every
   conformance marker present. A directory name never establishes artifact identity.

2. Build and freeze the distinct private-link cohort; leave faults in `build/`:

   ```sh
   worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt --out-dir build-link -- --locked --no-default-features --features link
   node verification/tooling/check-central-worker-bundle.ts link
   FROZEN_LINK="$EVIDENCE/frozen-link"
   mkdir "$FROZEN_LINK"
   cp crates/collaboration-central-cloudflare/build-link/index.js crates/collaboration-central-cloudflare/build-link/index_bg.wasm "$FROZEN_LINK/"
   ```

   Require zero exit, the link entry only and no conformance controls. Retain
   both link hashes and the source/template/configuration hashes.

3. Set `MINIFLARE_ENTRY` to the absolute entrypoint in this clone,
   `PERSISTENCE` to the fresh private directory, and `PORT` to the selected free
   port. Compare the actual faults bytes immediately before the consumer:

   ```sh
   cmp "$FROZEN_FAULTS/index.js" crates/collaboration-central-cloudflare/build/index.js
   cmp "$FROZEN_FAULTS/index_bg.wasm" crates/collaboration-central-cloudflare/build/index_bg.wasm
   cargo run --locked -p collaboration-central-tokio --features conformance --example organization_link_workerd -- node "$MINIFLARE_ENTRY" "$PERSISTENCE" "$PORT"
   ```

   Both comparisons must exit zero. Keep complete stdout/stderr and the parent
   exit code; avoid a pipeline that hides the Cargo exit. The example binds the
   port before launch and waits for the actual
   `Ready on http://127.0.0.1:<port> (real workerd, central-link binding)` line.
   A listening port alone supplies no readiness or correctness credit.

4. Require `LINK JOURNEY passed`, parent exit zero and every deciding
   `LINK CASE passed:` line. At the adopted reference there are **52** case
   groups. Retain the complete inventory; a count alone cannot substitute for a
   missing case. The harness drives real provisioning/replay/CAS races, apply-time
   expiry, key retirement, organization isolation, floor-abort before retry,
   person admin, closed private routes and an owned restart/readback. In particular:

   - `an aborted floor apply leaves the prior floor and never acknowledges`
   - `public central serves none of the private link routes, metadata included`
   - `B2 link routes on an unstamped legacy or non-legacy store refuse and reveal nothing`
   - `the conformance observer refuses another person's valid admin assertion`
   - `no Worker log line carries statement bytes, segments, digests or key IDs`

5. Require each `LINK HARNESS reaped pid=...` line, retain its recorded PID and
   exit (zero or 130), then read back absence of the exact recorded processes.
   The example restarts its own harness on the same persistence to check owning
   state survival. Keep persistence and logs until all consumers release them.
   An interrupted run needs exact owned-process cleanup and a named incomplete
   journey; do not kill by name or delete the evidence.

## B2c/B2d admin edges on both hosts — lasting

The native entry is
`crates/collaboration-central-tokio/tests/integration/organization_admin_edges.rs`,
through the real Unix link socket with signed person JWTs and DPoP. Run the
owning integration aggregate with per-case process isolation and retained names:

```sh
cargo nextest run --locked --profile ci -p collaboration-central-tokio --features test-support --test integration --no-fail-fast --status-level all --final-status-level all
```

Require zero exit, nonzero execution, each named PASS below, and explicit
passed/failed/skipped/filtered totals. The child-only
`process_restart::native_central_process` skip is distinct from executed cases.
Do not count filtered tests as runs or enable retries to hide a failure.

On workerd the preceding organization-link journey calls
`organization_link_workerd/admin_edge_cases.rs`; the setup line
`the edge-case organization is stamped and its floors are pushed` must pass
before its five semantic lines. Five cases cover six edges because the held
deadline case also proves the post-crypto/pre-transaction barrier.

All native names below have prefix `organization_admin_edges::`; all Worker
outcomes have prefix `LINK CASE passed: `.

| Obligation | Native deciding PASS name | Worker deciding outcome |
| --- | --- | --- |
| Independent token-exp, session and snapshot bounds; post-crypto hold, applying clock, equality expired, minus 1 ms applied | `a_held_request_is_refused_at_each_signed_deadline_and_applied_just_before` | `a held request is refused at each signed deadline with no marker or write, and applies one millisecond before` |
| Current authorization before replay, for a member and a subsequently revoked owner | `an_existing_operation_is_not_replayed_to_a_caller_not_authorized_now` | `an existing operation is refused, not replayed, to a member or to an owner revoked since` |
| Another currently authorized actor reuses the operation ID and receives Conflict | `another_authorized_person_reusing_an_operation_gets_a_conflict` | `a different authorized person reusing an operation gets the conflict, never the first answer` |
| Body changed under an unchanged signed intent is refused at verification; valid body applies | `a_body_changed_under_its_signed_intent_is_refused_before_admission` | `a body changed under its signed intent is refused at verification; the signed body applies` |
| Validly signed audience array omits the admin audience and is refused; named audience applies | `a_person_audience_array_without_the_admin_audience_is_refused` | `an audience array without the admin audience is refused; one that names it applies` |

Read the asserted answers, not just the labels: deadline refusal is
`{"result":"refused","reason":"expired"}`; cached unauthorized replay is
`notPermitted`; another authorized actor gets `conflict`; tamper and the missing
audience produce HTTP 401. Each deadline is isolated inside valid request, proof
and second-factor windows, with the other two signed deadlines later. The hold
acknowledges verification has completed before the Fleet clock advances.

**State limit:** both hosts' edge helpers project four array lengths: workspaces,
records, used requests and decisions. Refusal equality compares those counts;
the changed-actor conflict compares only workspace/record counts, since marker
admission is separate. Exact answer equality is asserted separately. None of
these count checks proves unchanged stored row values. The restart's separate
snapshot equality is not evidence for that stronger claim on every edge.

**Sensitivity limit:** native seeded failures isolate deadline enforcement,
applying-clock sampling, authorization before replay, changed-actor conflict,
intent binding and audience membership in the common core. Keep any such red's
source SHA, diff, intended failing assertion and byte-for-byte restoration
separate from the passing target. There is **no CF-specific seeded-red evidence**;
the Worker passes establish host wiring, not independently seeded CF sensitivity.
Do not start fault injection without its own commission.

## Production exclusion and remaining central checks — lasting

After every faults consumer has settled, build production into `build/` and
check it. This overwrites faults there, so preserve the frozen faults/link files
first and compare/copy the right cohort before any later consumer.

```sh
worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt -- --locked --no-default-features
node verification/tooling/check-central-worker-bundle.ts production
```

Require both exits zero and the entire conformance marker vector false.
`verification/tooling/check-central-worker-bundle.ts` owns this gate: it also
rejects retired paths, the test-only receiver, depth instrumentation and a link
entry masquerading as public central. Keep the actual checked JS/Wasm hashes.
Never alter the marker inventory or checker to obtain a passing result.

The link journey does not replace other assigned gates. The accepted central
collection also ran the five existing `composed_scope_workerd` modes:
`runtime-controls`, `composed-copy-suite`, `composed-transactions`,
`composed-numeric`, `composed-fleet-conflict`. Its command is:

```sh
cargo run --locked -p collaboration-central-tokio --features conformance --example composed_scope_workerd -- "$SCENARIO" node "$MINIFLARE_ENTRY" "$PERSISTENCE" "$PORT"
```

Use a fresh persistence directory/port and both faults comparisons for each;
require its named pass, parent exit zero and exact owned reaps. Run these before
the production overwrite. Keep required format, Clippy, native/Wasm, verification
package, owning unit/integration/E2E, SQLx and architecture gates whole; see
[Sunclaw setup](b2-sunclaw.md) for the observed environment and ratchet procedure.

## Return and boundaries

Return source/build/machine, tool and lock hashes, feature/profile selections,
each command/exit, named outcomes and counts, owning readbacks, before/after
source guards, bundle hashes/guard vectors, signatures and cleanup identities.
Retain failures alongside the separately qualified correction; a successful
replacement command does not erase the original red.

An actual issuer-to-Rust journey needs the separately declared joint sign-in
target and the paired Verifier. If assigned, build `central_sign_in_launcher`
once, sign it as [Sunclaw setup](b2-sunclaw.md) describes, and publish five measured
hashes (launcher plus faults/link JS and Wasm) with actual source/configuration
and signature evidence before that target is pinned. Reuse that exact cohort.
Local fixture passes do not establish real issuer session/absolute bounds,
browser/provider behavior, staging sign-in binding/provisioning or hosted
acceptance. Tokio-only InvalidConfiguration coverage is not a CF observation.
