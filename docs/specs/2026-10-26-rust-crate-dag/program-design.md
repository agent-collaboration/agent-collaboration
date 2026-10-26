# Program Design: crates form the dependency DAG, a ratchet guards the inside, lints set the baseline

Date: 2026-10-26. Author: 🦁 Fleet Lead. Mechanical structural change; the owner set the requirements directly (below), so there is no separate Requirements or Specification document.

## What the owner decided (the requirements this design realizes)

| Id | Owner decision (2026-10-26) |
|---|---|
| U1 | Rust dependencies form a DAG; lower layers never import higher ones. `domain-rust` states the rule. |
| U2 | Major boundaries are **crates**, as a few large bucket crates rather than many small ones, so the compiler enforces the DAG. |
| U3 | Inside a crate, a module-cycle check runs as a test, with no Python and no Dylint. |
| U4 | Add the important clippy lints to CI, chosen by measurement, with every hit fixed. |
| U5 | One PR, delivered with full testing, verification and green CI; Sol helpers in parallel. |

## Current system (measured at `b4881497`)

- **Workspace crates already form a DAG.** Cargo enforces it: 40 packages.
- **Inside crates there are 33 module cycles in 22 packages,** from cargo-modules' resolved `use` graph folded to top-level modules. The largest: `collaboration-central-core` (about 100 modules), `automation-storage` (35), `message-board` (23), `codex-router-proxy` (10). Source: the bake-off report at `tmp/arch-bakeoff/serial-full/report.json` in this worktree.
- **`collaboration-service` (about 50k production lines) has 4 cycles.** The largest is {mail_delivery_runtime, mail_push_delivery, notification_publisher, project_admission, push_record_delivery, service_identity}.
- **`ServiceIdentity` (`crates/collaboration-service/src/service_identity.rs:8`) is a service locator,** not an identity. It carries the Router ids plus every runtime collaborator, attached through about 25 `with_*` builders: mail runtime, admission, notification publisher, workers, stores, provider hubs, approval broker, journal. 19 production files in the crate take it, plus 5 in `codex-router-host` and 1 in `collaboration-mcp`. Any module holding it depends on the whole composition. **This is the main cause of the cycles.**
- **Layer test:** grouping modules by layer (contracts, stores, runtimes, projections, application, composition) leaves **28 upward module edges from 17 modules.** 11 point at `service_identity`; 7 at `collaboration_application`, through failure and budget types plus `mail_delivery_runtime/remote_wakes.rs:290` constructing `WakeOperations`. The rest are projection↔runtime placement and `project_store_registry → central_project_reads`.
- **Tool limits:**
  - cargo-modules resolves re-exports but misses fully qualified paths in function bodies and `super::` paths. `clippy::absolute_paths` doesn't close that gap: line 290 isn't flagged, and a `super::` cycle fixture passes.
  - `cargo modules --acyclic` checks items, not modules, and ignores filters.
  - arch-lint sees body paths but drops re-export edges and ignores `cfg`.

## Alternatives and selection

| Option | Gains | Costs | Verdict |
|---|---|---|---|
| A. In-crate checker only | Small change | Cargo enforces nothing; the checker has measured blind spots (body and `super::` paths) | Rejected (U2) |
| B. Split into many small crates | Maximum compiler enforcement | Large `pub` surface; more cross-crate generic copies (the compile-time doc, Finding 5); churn | Rejected (U2: "big bucket crates") |
| **C. Four bucket crates for `collaboration-service`, plus a ratchet test for in-crate cycles everywhere, plus a lint baseline** | The compiler enforces the main layering. The ratchet stops new cycles anywhere. Existing cycles shrink over time. | Dissolving the service locator is real refactoring across about 25 files | **Selected** |

Falsifiers that would reopen the choice:
- The four buckets don't compile without an upward edge the plan can't invert. Bring it back with the edge.
- The ratchet test's runtime exceeds 3 minutes in CI.
- A bucket crate's own release compile time grows by more than 20%, measured with Cargo `--timings` against today's.

## Target structure

### Bucket crates (replacing today's single `collaboration-service`)

```text
collaboration-service        L4 composition: builds the Router's collaboration graph at startup
  │  owns: CollaborationComposition (what ServiceIdentity's builders become),
  │        project_admission, notification_publisher, project_commit_publication, manifest_publication
  ▼
collaboration-application    L3 the API surface the MCP server and Host call
  │  owns: collaboration_application (the operations), board_views,
  │        central_project_reads, project_read_handle, the read projections
  ▼
collaboration-runtime        L2 workers, delivery routes, provider integration
  │  owns: mail_* / push_* delivery, schedule / run / summary / wake workers,
  │        provider_* / acp_* / codex_* / native_* / app_server_* routes, interaction broker,
  │        fleet_client, project_claim*, and any projection a runtime writes or reads
  ▼
collaboration-contracts      L0–1 shared vocabulary and stores
     owns: RouterIdentity (the identity half of ServiceIdentity), router_delivery_types,
           session_delivery_contract, native_delivery_result, scheduled_run_contract,
           machine_identity, message_effect_state, stored_*_receipt, endpoint_directory,
           project_store_registry/paths, the failure and result-budget types moved out of the application
```

Allowed edges: down only (L4→L3→L2→L1/L0, and skipping is allowed). Forbidden: anything upward. **Enforcement: Cargo**, because a crate cannot depend on a crate that depends on it. Existing external dependents (`codex-router-host`, `codex-router-cli`, `collaboration-mcp`, `board-legacy-upgrade`, `board-central-test-support`) import each item from the crate that owns it. This is a hard cutover: no re-export shim from `collaboration-service`.

### Binding table

| Concept | Owner | Home | Shape across boundaries | Kind |
|---|---|---|---|---|
| Router identity (service id, epoch, machine identity, installation peer, endpoints) | collaboration-contracts | `RouterIdentity` (new; the identity fields of today's `ServiceIdentity`) | Plain value, `Clone`; no collaborators inside | derived at startup |
| Runtime collaborators (mail runtime, admission, publisher, workers, stores, hubs, broker, journal) | collaboration-service | `CollaborationComposition` (modified; today's `ServiceIdentity` builders) | Not passed downward. Each component receives only the handles it uses, through its constructor. | composed at startup |
| Upward call: mail runtime applying remote wakes | collaboration-runtime defines; collaboration-application implements | trait `RemoteWakeControl` (new) in runtime, injected by composition | `async fn apply(&self, request: RemoteWakeRequest) -> RemoteWakeResult` (existing request and result types) | interface |
| Shared failures and budgets (`NativeSessionFailure*`, `ProviderInventoryFailure*`, `ScheduleOperationFailure`, `CollaborationRejection`, `MessageFailureKind`, `WakeWaitFailure`, `ResultByteBudget`) | collaboration-contracts | existing types, moved | unchanged shapes | type |
| In-crate module cycle baseline | workspace | `architecture/module-cycle-baseline.json` (new) | `{ "<package>": [[module, …], …] }`: sorted SCCs of top-level modules | persisted (repo file) |
| Lint baseline | workspace | `[workspace.lints.clippy]` in `Cargo.toml` plus `clippy.toml` | lint = "deny"; thresholds | config |

### The ratchet test (U3)

- **Home:** a new unpublished crate, `crates/workspace-architecture-check`, with one test `module_cycles_do_not_grow`.
- **What it does:** for every workspace package with a lib (or bin-only) target, it runs `cargo modules dependencies -p <pkg> --lib --no-externs --no-sysroot --no-owns`, parses the DOT `uses` edges, and folds each item to `crate::<top-level module>`. It drops edges within a module and edges to the crate root, then computes SCCs. Packages run four at a time.
- **Rules:**
  - Fail if any package has an SCC that isn't covered by a baseline SCC, meaning a new cycle or a module joining one. Print the concrete item edges.
  - Fail if a baseline SCC no longer exists or has shrunk. The message says to update the baseline, so it can only get smaller.
- **Where it runs:** `#[ignore = "needs cargo-modules; run by CI's static job"]` keeps a local `cargo test --workspace` fast and tool-free. The CI static job installs cargo-modules (pinned 0.28.0) and runs it with `cargo nextest run -p workspace-architecture-check --run-ignored only`. Locally it's one command, documented in AGENTS.md.
- **Known blind spots, recorded rather than hidden:** fully qualified body paths and `super::` paths. Crate boundaries cover the cross-bucket case completely. Inside a bucket, the ratchet is best-effort.

### Lint baseline (U4, chosen from tonight's measurement)

Deny, fixing every hit:
- `wildcard_imports` (468 production hits);
- `allow_attributes` and `allow_attributes_without_reason` (180 and 198): every suppression becomes `#[expect(lint, reason = "…")]`;
- `large_futures` (46, default 16 KiB threshold);
- `cast_possible_truncation`, `cast_sign_loss`, `cast_possible_wrap` (87, 10, 5);
- `match_wildcard_for_single_variants` (17), `unwrap_in_result` (9), `unused_async` (12), `trivially_copy_pass_by_ref` (4), `large_stack_arrays` (1);
- already at zero: `large_types_passed_by_value`, `large_stack_frames`, `enum_glob_use`, `dbg_macro`, `implicit_clone`, `inefficient_to_string`.

Not adopted, with reasons:
- `missing_errors_doc` (1,697) and `must_use_candidate`: documentation noise;
- `print_stdout` and `print_stderr`: our CLIs print on purpose;
- `wildcard_enum_match_arm` (531): needed for non-exhaustive foreign enums; a follow-up for our own domain enums;
- `too_many_lines`: needs a measured threshold;
- `needless_pass_by_value`, `future_not_send`, `cognitive_complexity`: noisy.

## Failure and behaviour

The behaviour change is zero: types and functions move, constructors take narrower handles, and one call goes through a trait whose implementation is the existing `WakeOperations` path. The proof is the unchanged workspace suite (about 5,580 tests) plus E2E, passing at the final head. No stored data, wire format or public CLI or MCP contract changes.

## Trace

| U | Realization | Owner | Proof seam |
|---|---|---|---|
| U1 | The `domain-rust` rule: dependencies point one way; shared types live low; upward needs become injected traits; layers are crates | sunstack `domain-rust` | Skill review; the rule names this design as its example |
| U2 | Four bucket crates, with `ServiceIdentity` split into `RouterIdentity` plus `CollaborationComposition`, `RemoteWakeControl`, and failure types moved down | the bucket crates | `cargo check --workspace` (Cargo forbids upward crate edges); the full test suite unchanged |
| U3 | `workspace-architecture-check` ratchet with `architecture/module-cycle-baseline.json` | the workspace check crate | It fails on a planted two-module cycle; it fails when a module joins a baseline SCC; it passes on the PR; it runs in CI |
| U4 | `[workspace.lints.clippy]` denies plus thresholds | workspace `Cargo.toml` | Clippy `-D warnings` green in CI; hit counts before and after in the PR |
| U5 | One PR, parallel Sol lanes, independent review | Lead | CI links, review verdict, this trace |

## Deviations and unresolved

- **The cross-bucket graph misses body paths.** Splitting into crates makes the compiler report every remaining upward edge, so the plan expects a few more edges than the 28 measured.
- **The other 32 in-crate cycles are frozen by the baseline, not fixed, in this PR.**
- **`collaboration-central-core`'s 100-module cycle** is the next candidate for a bucket split. It's a follow-up.
