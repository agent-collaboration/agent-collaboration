# Central automations across two machines

This lasting journey is **draft**. Its model-free bootstrap prepares two real,
separately enrolled local Hosts and a real Tokio central. It sends no model input
and establishes no automation requirement. Full signed Router proxies, provider
sessions, the native no-input create path, causal clock/fault controls, and
V-row collectors await the declared product milestones.

The oracle is the four-document set in
`docs/specs/2026-10-31-central-automations/` and its
`plans/2026-10-31-central-automations-delivery.md`. The milestone gates table
decides the target and passing condition; a stand-in or apparatus test supplies
no product credit. Confirm the adopted revision before each batch.

## Bootstrap

Use the assigned worktree throughout. Keep builds in its normal `target/debug`;
keep runtime state and evidence in separate, fresh, private directories under
that worktree's `tmp`. A short root such as `tmp/r` leaves room for macOS socket
paths. No production Router directory or copied live data is an input.

Build and sign the verification-only example:

```sh
df -h .
cargo build --locked -p codex-router-host --example central-automations-rig
scripts/cargo_debug_signing_runner.sh --sign-only target/debug/examples/central-automations-rig
```

The example's `--help` describes three process modes: `central`, `host-a` and
`host-b`. Its central mode reuses `TestCentral::start_with_peer` and
`additional_machine`; each Host reuses `CollaborationRuntime` and its own
persisted installation identity. Enrollment uses local synthetic Access
credentials, never a deployed account. The bootstrap leaves Claude disabled and
the native backend unavailable.

Create a plan using `centralAutomationsPlanSchema` in
`verification/observations/central-automations-plan.ts`. It requires:

- the canonical `repository` and exact `sourceHead`;
- the normal compiled rig `executable` path, SHA256, actual signed identifier
  and team;
- fresh `directory` and `evidence` paths under `repository/tmp`, with canonical
  existing parents and no overlap;
- `purpose: "bootstrap-only"`, `model: "claude-haiku-5-5"`, and a positive
  `maxBudgetUsd` no greater than 0.25. The bootstrap spends none of it.

Inspect the artifact's actual signature and hash when constructing the plan.
The signing runner's test identifiers differ from released Router identifiers.
No previous machine's plan, identities or hash qualifies this build.

```sh
node verification/observations/central-automations-rig.ts --help
node verification/observations/central-automations-rig.ts "$RIG_PLAN"
```

The driver qualifies the artifact and source, starts central, then Host A and
Host B in separate owned processes. It waits for their structured ready records,
then reads both installations through their real MCP endpoints. Readiness must
name the actual child PID; installations must be distinct. `ready.json` records
that setup and explicitly contains zero observed product rows and zero inputs.

The driver shuts down Host B and A, then central, and waits for each exact child
to exit. `settlement.json` retains exit/signal results and cleanup failures.
Keep stdout, stderr, declaration, readiness and settlement together. A failed
launch is setup evidence; it is never a failing product case. No missing or
empty scenario can become a pass.

## Product observations after target declaration

Before driving a gate, bind its source commit, signed artifact hashes, config,
client and fixtures. Read the offered tests against each wrong build in the
Verification Design. Retain the deciding business effects, not only calls or
receipts. Use these boundaries:

| Gate | Required deciding observation |
| --- | --- |
| M0 | Fresh migrated synthetic stores: per-store WAL, old/new retention rows, reclaim, route import and owning restart; valid and corrupt JSON fixtures. Never read live account data. |
| M1 | Each R5 limit across shared validators, persisted decode and actual CLI/MCP publication; V6 constructor/property cases. No central acceptance credit yet. |
| M2 | Shared scenarios through the central contract client on **both** Tokio and workerd: catalog, no lease gate, replay/conflict, grid, alarm/error recovery, batch conservation/stable bytes/custody and sequence ordering. |
| M3/M4 | Real A/B CLI and MCP, offline recovery, local receipt admission, frozen content and run continuity; central cutover and rejection of the retired remote-wake route. |
| M3b | V18 provider create/input and real Chief permission decision/callback; no-Chief refusal; offline/replay/unknown/partial settings. Codex create without model input is a separate required observation. |
| M5 | V14 converted executable occurrence sets and suspended legacy crons, stored first due/watermark, uncertain external occupant and post-import settlement; old release and identical history. |
| M6 | V7 rolling cooldown across restart, 3 projects/3 kinds, overflow and authorized member page, immediate non-notification traffic, Off/Poll and remote replication. |
| M7/M8 | V17 part 1 scans S9's own commits against their routed base at M7. Part 2 scans PR 2's single squash commit and tree at merge; a non-squash merge fails. Never call the earlier pushed branch ancestry clean. M8 runs every product row together at the integrated head, retaining the unobserved merge part separately. |

Clock advancement must return after the actual due scans, `on_alarm` and
`AutomationClock::evaluate` finish. A SetTime acknowledgement alone proves none
of them. Arm a named fault point and await its held signal with reached operation
and native acceptance IDs before each kill. Use release/stop/exit barriers and
injected clocks; bounded failure guards do not authorize sleeps or polling.

Every wrong-build edit stays uncommitted in the verifier's own working tree.
Retain its diff, exact command, intended failing assertion and exit. Restore the
original bytes and rerun the affected green gate. A break introduced after the
implementation establishes sensitivity; it does not establish tests-first order.

Product repairs and public/persisted contract decisions belong to the assigned
implementation lane and Lead. Preserve an unknown result and its IDs when an
acceptance answer is lost; never repeat a create or input to make the result
easier to observe.

## Apparatus checks

```sh
cargo check --locked -p codex-router-host --example central-automations-rig
cargo test --locked -p codex-router-host --example central-automations-rig
cargo clippy --locked -p codex-router-host --all-targets -- -D warnings
cargo fmt --all -- --check
pnpm --dir verification run check
pnpm --dir verification run lint
pnpm --dir verification run format:check
pnpm --dir verification test
```

These qualify apparatus only. Report each product claim as supported,
unsupported or unknown at its declared target, with build/config/client,
commands/counts/exits, raw evidence paths and every remaining setup or coverage
gap. Milestone acceptance remains with the Lead.
