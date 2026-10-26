# Program Design: crates form the dependency DAG, a ratchet guards the inside, lints set the baseline

Date: 2026-10-26 (revised the same night after the independent design review and the Advisor). Author: 🦁 Fleet Lead. Mechanical structural change; the owner set the requirements directly (below), so there is no separate Requirements or Specification document.

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

## Alternatives and selection (revised after the design review and the Advisor, 2026-10-26)

| Option | Gains | Costs | Verdict |
|---|---|---|---|
| A. In-crate checker only | Small change | Cargo enforces nothing; the checker has measured blind spots | Rejected (U2) |
| B. Many small crates | Maximum compiler enforcement | Large `pub` surface; more cross-crate generic copies; churn | Rejected (U2: big buckets) |
| C1. Four crates, including a separate composition crate | An extra boundary | The review found it puts live policy (admission, publication) above its callers, so it can't compile as a DAG. The composition role already lives in the Host's `startup_composition`, so a fourth crate is a pass-through layer (justify-the-complexity). | Rejected |
| **C2. Three crates: `collaboration-contracts` ← `collaboration-runtime` ← `collaboration-service` (the application), with composition staying in `codex-router-host`, plus the ratchet and the lints** | The compiler enforces the main layering, with fewer moved APIs. The application crate keeps its name, so the import churn for the Host, MCP and CLI is small. | Admission, publication, subscriptions and mail coordination stay together in the application crate, and the ratchet and module discipline guard inside it | **Selected** |

Falsifiers:
- A placed module needs an upward edge beyond the 3 handled in `module-placement.md`.
- A family constructor can't be written without passing a services bag.
- The ratchet takes more than 3 minutes in CI.

## Target structure

```text
codex-router-host            COMPOSITION (existing startup_composition.rs): builds every component once and
  │                          injects handles. The only place that sees everything.
  ▼
collaboration-service        APPLICATION: the operations the MCP server and Host call, and the policies they decide.
  │  owns: collaboration_application/* operation families, board_views, project_admission, notification_publisher,
  │        project_commit_publication, subscription_delivery (hoisted out of service_identity), mail coordination
  │        (mail_delivery_runtime, mail_push_delivery, mail_push_outcome), session and provider inventories,
  │        schedule preparation and activation, push-record resolution and delivery, public projections
  ▼
collaboration-runtime        EXECUTION: resource-backed capabilities and the workers that own them.
  │  owns: the project store family (registry, paths, claim store, read handles, central reads), endpoint_directory,
  │        fleet_client, provider and ACP/app-server/native adapters and stores, delivery routing, workers (schedule
  │        timing, scheduled runs, summary, wake timing, retention, push handoff), mail sessions and persistence
  │        hooks, persisted receipt codecs, service identity storage
  ▼
collaboration-contracts      VOCABULARY and PORTS: values and capability interfaces shared across the layers. No mutable owners.
     owns: RouterIdentity, machine_identity, session_delivery_contract, native_route_claim, native_delivery_result,
           router_delivery_types, scheduled_run_contract, session_command_port, provider_conversation_backend (port),
           project_claim and project_registration (Host-implemented ports), the project_replication_position port
```

The full per-module placement for all 117 modules is in `module-placement.md`. Cargo enforces: contracts depends on neither runtime nor application; runtime doesn't depend on the application; nothing in the collaboration crates depends on the Host.

## Dependency injection (justify-the-complexity, separate-the-concerns)

- **`RouterIdentity`** (contracts) holds only immutable identity facts: service id, service epoch, installation peer and machine identity. The live `EndpointDirectory` is shared mutable state, a separate runtime handle that's injected where it's used.
- **`ServiceIdentity` is deleted.** The Host's `startup_composition` constructs each component and each application operation family with exactly the handles it uses, as constructor arguments. There's no services struct and no composition object passed down. A component with one behaviour and several inputs (for example `SubscriptionDeliveryServiceProps`) is fine. The rule is: no exposed bag from which a consumer picks unrelated services.
- **Large families are split into behavioural components, not bags.** Board and Sessions honestly need about ten inputs. Board's session-context reader receives a session-inspection component, not the session runtime's raw inputs.
- Per-family inputs, from the review's source reading:
  - Board needs admission, claim, publication/subscription, presence/clock, the interaction store, and the session-context component.
  - ViewSnapshot needs the registry and replication.
  - Message needs the interaction store, mail coordination, the display-name cache, identity, and the registry for activity expansion.
  - Conversation needs the provider conversation backend, the provider operation store, the conversation recorder, the endpoint directory and identity.
  - Observation needs the session event hub, the endpoint directory and identity.
  - Session and Codex inventory need the endpoint directory, the native control backend and gate, the broker, the display-name cache and the lifecycle journal.
  - Provider inventory needs the endpoint directory, the provider operation store, the event hub, the display-name cache, identity, and the Claude readers for the native catalogue only.
  - Automation needs the automation store, configuration handle and backend, session message delivery, the interaction store, scheduled-run execution, an optional native control backend and identity.
  - Wake needs identity, the automation store and the ONE shared wait semaphore; outbound remote routing additionally gets mail coordination.
  - Interaction needs the broker.
  - Journal needs the lifecycle store, the endpoint directory and identity.
- **Startup contracts that today live as hidden builder side effects must be kept, and tested where they aren't already:**
  - mail attachment checks physical `InteractionStore` identity with `Arc::ptr_eq` (`mail_delivery_runtime.rs:281`), and attaches the same wake store, permits, readiness and replication (`service_identity.rs:48–70`);
  - follow admission holds `Weak` handles to admission and the publisher (`service_identity.rs:141`);
  - the approval broker wiring installs display names and push context on the physical interaction store (`service_identity.rs:331`);
  - optional and degraded subsystems stay optional; an `Option` must not turn into a new mandatory dependency.
  Nothing creates a second semaphore, cache or store per family.
- **No new traits.** `RemoteWakeControl` is dropped: mail coordination and wake operations share the application crate, so the existing call stays a direct call, and the review's deletion test passes. `CollaborationRejection` and its foreign-type implementations stay in the application; only data types lower layers truly need move down.

### Binding table

| Concept | Owner | Home | Shape | Kind |
|---|---|---|---|---|
| Router identity | collaboration-contracts | `RouterIdentity` (new, `router_identity.rs`) | Validated values; `Clone` | derived at startup |
| Live endpoint inventory | collaboration-runtime | `EndpointDirectory` (existing) | Shared handle, injected separately | live state |
| Wiring | codex-router-host | `startup_composition.rs` (existing, extended) | Constructs and injects; never passed down | composed at startup |
| Operation families | collaboration-service | the existing `*Operations` types | Each constructed with its own handles | components |
| Module-cycle baseline | workspace | `architecture/module-cycle-baseline.json` | Per package: SCC module sets plus their concrete module edges | repo file |
| Crate layer policy | workspace | a declared table in the `workspace-architecture-check` test | allowed normal and build edges between the collaboration crates and the Host | test |
| Lint baseline | workspace | `[workspace.lints.clippy]` plus `clippy.toml` | deny | config |

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

## Principles applied (read in full, 2026-10-26)

- **justify-the-complexity:**
  - "a home-grown container or service locator where passing dependencies as arguments would do" is today's `ServiceIdentity`, and the defect this design removes.
  - "a constructor argument beats a home-grown dependency-injection container", so the injection is plain constructor arguments, with no framework and no services struct.
  - Cut over completely: every caller is migrated in the same change, and there is no re-export shim from `collaboration-service`. The caller inventory is the field map: 30 `pub(crate)` fields, with each reader listed by file.
- **separate-the-concerns:**
  - "Hand each part only what it uses": constructor arguments come from the field map.
  - "Inject at real seams": a trait exists only where a lower crate must call a higher one (`RemoteWakeControl`, plus admission's read of board views if it isn't a pure read that can move down). Downward collaborators with one implementation stay concrete types, never traits made for symmetry.
  - Interfaces are defined from the consumer's side.
- **one-owner-per-truth:**
  - The Router identity is owned by `RouterIdentity` in contracts.
  - Wiring is owned by composition.
  - Each collaborator is owned by the crate that implements it.
  - No component writes another's state through a shared bag.
- **types-are-communication:** `RouterIdentity` keeps its typed ids. Moved failure types keep their closed variants, and nothing becomes a string.
- **domain-rust:**
  - New modules get two- or three-word responsibility names, never `types.rs`, `util.rs` or `common.rs`. The contracts crate's modules are named for their contract (`delivery_contract`, `router_identity`…).
  - Errors stay `thiserror` enums at each crate's edge.
  - No production `unwrap`.

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
