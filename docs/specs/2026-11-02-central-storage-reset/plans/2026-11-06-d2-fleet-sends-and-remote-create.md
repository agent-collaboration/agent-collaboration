# Plan: D2, Fleet sends and remote create

## Canonical record

```text
plan path: docs/specs/2026-11-02-central-storage-reset/plans/2026-11-06-d2-fleet-sends-and-remote-create.md
originating planner: plan-implementation
supersedes: none
planning result: ready (code starts only after the owner approves the three D2 tables, tmp/2026-11-05-after-d1-decisions.html section 3)
governing planning basis:
  kind: reviewed-four-document-design
  Requirements: docs/specs/2026-11-02-central-storage-reset/requirements/index.md (U8, U10)
  Specification: docs/specs/2026-11-02-central-storage-reset/specification/remote-sessions.md (R16–R22), entities.md (E10)
  Program Design: docs/specs/2026-11-02-central-storage-reset/program-design/d2-fleet-sends-and-remote-create.md
  Verification Design: docs/specs/2026-11-02-central-storage-reset/verification-design/d2-fleet-sends-and-remote-create.md
  review: tmp/d2-review/2026-11-04-d2-review.md (9) → codex-router.review-d1-pr215/tmp/review/2026-11-06-d2-r2-design-review.md (6) → 2026-11-06-d2-r3-design-review.md (1) → 2026-11-06-d2-r4-closure.md (1) → 2026-11-06-d2-r5-closure.md (0, closed at e159cae14)
  applicability anchors: codex-router origin/main 9ad3c0db0 (0.1.126 released line; 0.1.127 and 0.1.128 reserved by open PRs); agent-fleet-sync origin/main 30371bf
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:d2-two-lanes
  PR topology: one-pr per repository (the D1 precedent: one deliverable, one PR; the library PR merges and tags first because the Router pins it by git tag)
  repositories: agent-collaboration/agent-fleet-sync, agent-collaboration/agent-collaboration (codex-router)
planned at: design/central-storage-reset @ e159cae14
```

## Goal

From a session on machine A, `agent-collaboration conversation create --machine <B> …` starts one session on B through the Fleet, at most once, with at most one first input, and the requester gets exactly one result message from the Fleet: the new session, a typed refusal, "not sure", or "expired". The Fleet is a real mail sender that no machine can impersonate.

**Not in scope:** workspace membership and permissions; Fleet messages from automations (D4a, D4b); queuing for an offline B (temporary refusal, R18); approvals routed through central (owner's approvals step 3).

## Lanes and order

Two lanes, each an 🐒 Implementer on Sol 6.1 high with a paired 🔬 Verifier on a different lineage where available. Lanes work in separate worktrees, so their Rust builds don't serialize.

```text
Central lane:  L1 ──► C1 ──────────────┐
Router lane:   S1 ──► R1 ──► R2 ───────┼──► G1 (Tokio journeys) ──► G2 (staging subset)
               (S1 is consumed by C1 too)┘
```

| Edge | Why |
|---|---|
| L1 → C1, L1 → R1 | both sender bindings resolve `MailSender::Fleet` to `Submitter::Fleet` |
| S1 → C1, S1 → R1, S1 → R2 | every slice uses the closed shapes and the address type |
| C1 → G1, R1 → G1, R2 → G1 | first point where the request handler, command owner and requester meet |
| G1 → G2 | staging runs only the subset the VD names, on a build that passed Tokio |

C1 and R1 run in parallel once L1 and S1 land. S1 is one small slice the Router lane does first; the Central lane starts L1 at the same time.

## Slices

### L1 (library, contract): typed Fleet submitter

- **Reads:** PD "Components and changes", row *Typed Fleet submitter*; `fleet-sync-protocol/src/submitter.rs`, `fleet-sync-delivery/src/submitter.rs`, `delivery_storage_rows/relay_submitter.rs`.
- **Change:** `Submitter::Fleet(FleetId)`; its stored round trip as a third `kind`; an internal Fleet admission entry separate from machine submit, asserting `Submitter::Fleet`.
- **Wrong behavior the proof fails on:** a Fleet submitter that doesn't survive payload release and reopen; machine submit accepting a Fleet submitter.
- **Proof:** extend `tests/integration/typed_submitter.rs` (keep) with the Fleet round trip through release and reopen, and a machine-submit refusal. Oracle: PD row. Then library CI, merge, tag.

### S1 (Router, contract): mail shapes and address

- **Reads:** PD "Closed shapes", "Contract mapping and bounds", rows *Delivery address* and *Bodies and bounds*; `collaboration-mail-protocol/src/mail_content.rs`, `remote_wake.rs`; `board-sync-protocol/src/board_delivery.rs`.
- **Change:** `MailSender::Fleet`; new `remote_create.rs` with every closed type (`RemoteCreateRequest` … `RemoteCreateOutcome`, `CreateRefusal`, `PromptFailure`, `SettingName`, bounded code and text); bodies `CreateSession`, `RemoteCreateResult`; untagged `DeliveryAddress = Session | Machine { machine }`; `max_acceptance_bytes_for(CreateSession) = 64 KiB`; `validate_sender` accepts the new bodies only from `Fleet`; the allowed sender/body/address combinations.
- **Wrong behavior:** an old bare `SessionRef` address that no longer decodes; a value that decodes as both address shapes; an unknown variant accepted; a non-Fleet sender with `CreateSession` accepted.
- **Proof:** a table test over every allowed and refused combination (one rule, so a table, not examples); fixtures with D1's retained address bytes decode as `Session` (the unit half of P12); worst-case `CreateSessionResult` fits 64 KiB. Oracle: PD shapes and the combination list.

### C1 (central): request handler, status, result

- **Reads:** PD rows *Fleet request handler*, *Fleet status query*, *Fleet completion observer*, Tables `fleet_remote_creates`, "Failure cases"; D1 PD "Rules added during delivery" (enrollment gate); `collaboration-central-core/src/mail_sender_authority.rs`, `mail_submit_admission.rs`, `fleet_mail_tasks.rs`, `effect_driver/fleet_requests.rs`.
- **Change:** `fleet_remote_creates` (UUIDv7 key = request id, typed columns, timestamps, CHECK only on booleans); the request call with replay equality over requester, target and digest; enrollment and online checks with the temporary-rule comments linking R17 and R18; internal Fleet admission of the command; the status query; the completion observer minting `result_item_id` in the report's transaction; the 60-day removal admitting `Expired` the same way; `FleetSenderBinding`; machine submit refusing a Fleet sender.
- **Wrong behavior:** a second result on a retried report; the result id minted at admission; a changed target or requester replayed instead of refused; a machine submitting as the Fleet.
- **Proof:** central-core tests per rule, then conformance scenarios on both backends (Tokio and local workerd), as D1 did. Earns the central half of P4, P5, P8, P8b, P11, P11b, P14.

### R1 (Router, machine B): command owner

- **Reads:** PD rows *B command owner*, *Creator and approver on B*, *First input*, *Report outcome*, Tables `mail_create_commands` and the approver table, the refusal mapping table; `collaboration-runtime/src/received_mail_push.rs`, `collaboration-service/src/collaboration_application/conversation_operations.rs`, `codex-router-host/src/external_provider_supervisor.rs`, `conversation_create_actor.rs`, `agent-collaboration/src/approval_commands.rs`, `interaction-storage/migrations/`.
- **Change:** migrations for `mail_create_commands` and `approval_remote_create_approver`; `agent-collaboration approvals remote-approver set|clear` with local-session validation; the receive branch before projection; `MachineSenderBinding` resolving the enrolled Fleet; local outbox refusing a Fleet sender; `mail_create_command.rs` with ownership-before-adoption (r5), the approver read, both create owners, Codex `dispatching` before the prompt, effect classification, and the Report acceptance.
- **Wrong behavior:** two sessions for one request; adopting an operation it didn't admit (P15 a, b); a second prompt after a crash; `accepted` for a refused prompt; a create with no named approver.
- **Proof:** service tests per rule with a staged provider store; Host integration tests for the restart cases with named crash points (debug/conformance only, as D1's controls). Earns P2, P3, P6, P9, P13, P15 at the local layer.

### R2 (Router, machine A): requester surfaces

- **Reads:** PD rows *CLI and MCP*, *Recipient projection*, "What you will be able to do".
- **Change:** the request call over A's machine socket with A minting the request id and reusing it on retry; `conversation create --machine`; MCP `conversation_create` with `machine`; `conversation remote-status --request-id`; `PushOrigin::Fleet` with pushes labelled "Fleet".
- **Wrong behavior:** a retry minting a new id; a caller able to supply a remote-requester actor or a `Machine` address (P10).
- **Proof:** CLI integration tests (keep and extend the existing conversation-create CLI tests); one MCP case.

### G1 (integration): Tokio journeys

The paired Verifiers build the harness extensions the VD names on `verification/observations/central/d1-harness.ts` (approval-enabled B with a named approver, A disconnect and reconnect, the injected mail clock, a pre-seeded B operation and a staged pending row), through `verify-codex-router`. Then they run P1–P15 on the three-Router Tokio harness, P1 once with `codex-local` and once with `claude-local` (debug Router only; the Claude provider runs on Haiku, and the owner is asked before that run). This is the first gate where C1, R1 and R2 meet; it runs as soon as all three land.

### G2 (staging): the Cloudflare subset

After the D2 redeploy through the fleet-ops D1 redeploy mode: P1 (`codex-local`), P4, P5, P10, P13. Tokio results don't count as Cloudflare proof. P3's crash points stay native-only.

## Obligation to proof

| VD row | Slice and layer | Runs |
|---|---|---|
| P1, P1 MCP case | G1 (both create owners), G2 | after R2 |
| P2, P3 | R1 local; G1 | R1 tests; G1 |
| P4, P5 | C1 conformance; G1; G2 | C1; G1; G2 |
| P6, P9 | R1; G1 | R1; G1 |
| P7 | C1 (comment and test name) | C1 review |
| P8, P8b | C1; G1 | C1; G1 |
| P10 | S1 table; R2; G1; G2 | S1; R2; G1; G2 |
| P11, P11b | G1 readbacks | G1 |
| P12 | S1 fixture; G1 retained mail | S1; G1 |
| P13 | R1; G1; G2 | R1; G1; G2 |
| P14 | C1; G1 | C1; G1 |
| P15 (a–d) | R1; G1 | R1; G1 |

## Gates for each PR

Per package, never multi-package full runs on shared Macs: `cargo nextest` for each changed package through the repo runner, clippy `-D warnings`, fmt, the module-cycle ratchet, `pnpm --dir verification` check/lint/format/test, both conformance backends, then CI. Version: the next unused workspace version at merge, with `Cargo.lock`. One independent Sol xhigh review per PR before merge.

## Existing tests

| Test | Disposition |
|---|---|
| `fleet-sync-delivery/tests/integration/typed_submitter.rs` | keep, extend |
| `collaboration-runtime/src/received_mail_push_tests.rs` | keep; add the `Machine` branch |
| `codex-router-host/tests/integration/remote_wake.rs` | keep (same branch pattern, unchanged) |
| D1 harness tests under `verification/observations/central/` | keep; extend for D2 |
| `agent-collaboration/tests/integration/approval_list_cli.rs` | keep; remote-approver gets its own cases |

No test is removed.

## Risks

- **False green:** a P15 test that seeds the provider store without the real admission path passes for a build that adopts by id. R1's P15 cases go through the real `prepare_operation` and Codex create owner.
- **False green:** P14 on an empty Fleet passes for an early-minted result id. The test reads the result's UUIDv7 time against the command's deadline.
- **Approver gate:** if `codex_create_actors` or the provider local-actor gate rejects the named Chief of Staff session, the PD's "passes both gates unchanged" claim is wrong. Stop R1 and bring the evidence.
- **Workerd:** if internal Fleet admission can't run inside the report's transaction on the Worker, stop C1; the "same transaction" rule is the design.

## Stop and replan

- Any stored table or column beyond the three the owner approved.
- A change to the D1 mail wire format other than the additive address and body variants.
- The Codex prompt call can't separate a pre-effect refusal from a possible effect (P13 would have nothing to observe).
- A slice needs a product decision the design doesn't make.
