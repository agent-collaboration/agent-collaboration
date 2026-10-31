# Specification: central automations

Traces to [requirements.md](requirements.md) (U1–U11). The capability keys are those defined there.

## Problem and current reality (main, 2026-10-31)

- **Definitions are per machine.** Each Router keeps its schedules and wakes in its own `automation.sqlite`. There's no project column and no central copy.
- **Firing is local.** The machine that holds the definition keeps its timer (`schedule_timing_worker`, `wakeup_timing_worker`).
- **Delivery is a local push.** A fired wake or schedule run is delivered as a Router push through that machine's `interaction.sqlite` (`wakeup_delivery_sender`, `scheduled_run_worker`).
- **Only one path crosses machines.** A wake aimed at another machine's session is sent as fleet mail to the machine that owns the target. That machine then holds the definition and the timer (`wake_operations/remote.rs`).
- **The consequence:** an automation is visible only on its own machine, stops when that machine is off, and can't be managed from elsewhere.
- **The published spec is stale.** It still says a project's wakes and schedules pause when its lease expires. The owner removed that rule (U3), but the removal wasn't published (U11).

## Entities

| ID | Term | Identity rule | Relationships | Invariants | Observable states | Canonicalizes | Basis |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E1 | **Automation** | its automation ID, minted once when it's created and kept through every change and through migration (R11) | belongs to exactly one Fleet (E8); is exactly one kind, Schedule (E2) or Wake (E3); has 0..n Firings (E4) | not tied to any project (U3); one definition, one place (U2) | per kind (E2, E3) | "automation", "schedule", "wake", "timer"; excludes the delivered message (E5) | U1, U2, U3 |
| E2 | **Schedule** | an Automation of kind schedule | has one timing and one instruction, plus everything today's schedule definition carries: model, effort, execution timeout, and one destination, either an existing session (reuse) or a machine endpoint plus folder (fresh each run). The move keeps all of it (`collaboration-protocol/src/schedule_contract.rs:11–34`) | at most one occupying Run (E6) and one waiting Run; further due times fold into the waiting Run (today's rule, carried forward) | enabled, disabled, deleted | "scheduled workflow", "schedule run definition" | U1, U4, U7 |
| E3 | **Wake** | an Automation of kind wake | has one target session, one due time, an optional repeat with an end, and a creator | the U66 limits; at most one active Wake per target session; while a Firing of it is pending, new due times fold into that Firing (today's rule, carried forward) | active, paused, ended (expired, cancelled or completed) | "wake-up", "reminder", "wakeup definition" | U1, U4 |
| E4 | **Firing** | one Automation plus one due time; a due time that folds into a pending Firing joins that Firing and makes no new one | belongs to one Automation; is carried by at most one Delivery item (E5); yields at most one Run (E6) for a Schedule | decided by the Fleet's clock (U5); carried out at most once (U7) | due, held, delivered, run started, run finished, failed, expired, cancelled | "occurrence", "fire", "attempt"; excludes retries of the same Firing's delivery, which stay the same Firing | U5, U6, U7, U8 |
| E5 | **Delivery item** | the message item that carries one Firing; the same identity rules as any message item (U63) | carries exactly one Firing to exactly one target machine | travels outbox-free from the central ledger to the target machine's inbox, then into local delivery; held while undelivered; "valid until" honoured | the U63 states (queued, sent, at machine, held, delivered, uncertain, expired, cancelled, refused) | "mail item", "push" | U6 |
| E6 | **Run** | one Schedule Firing carried out on its target machine | belongs to one Firing; runs in exactly one session (the bound one, or one it started fresh) | one per Firing, never two | started, finished (with summary), failed, cancelled | "workflow run", "scheduled run" | U7, U8 |
| E7 | **Firing outcome** | the latest observable state of one Firing | belongs to one Firing; readable from any Machine (E9) in the Fleet | after a state is acknowledged, a later read from any machine never shows an earlier one | as E4 | "result", "status" | U8 |
| E8 | **Fleet** | the set of machines enrolled with one central deployment (the existing enrollment meaning) | has 0..n Automations and 1..n Machines | holds the one list of Automations (U2) | (unchanged) | "central", "fabric" | U2, U5 |
| E9 | **Machine** | an enrolled Router installation (the existing meaning) | belongs to one Fleet; hosts sessions; receives Delivery items | carries out the Firings delivered to it | online or offline (from central's view) | "installation", "host" | U6, U7 |
| E10 | **Local automation history** | the run and delivery records a Machine kept before migration | belongs to one Machine | read-only after migration; removed by the 2-month retention | readable, then removed | "automation.sqlite history" | U10 |

## Obligations

### definitions

- **R1, one list everywhere** (U2). From any Machine in the Fleet, the Router CLI and MCP create, list, show, change, pause, resume and cancel Automations.
  - Once a write is acknowledged, any later read from any Machine shows it.
  - Two Machines listing at the same moment, with no write in between, show the same Automations and states.
  - There's no per-machine list.
- **R2, no project** (U3). An Automation carries no project. A project's lease state, including expiry, never stops, delays or pauses a Firing. The board-orchestration obligations that pause wakes and schedules on lease expiry are void, and are removed by R15.
- **R3, the wake rules hold at the single place** (U4). Creating or changing a Wake enforces the U66 limits and the one-active-Wake-per-session rule against the Fleet-wide list. A violation is refused with the existing typed refusals (`wakeExists` naming the creator, and the limit refusals). The creator is recorded and shown.
- **R4, writes need central** (U2, with board design U80: "a machine works only while it's online").
  - A create, change, pause, resume or cancel issued while central is unreachable is **refused visibly**, naming central as unreachable. It's never queued as an offline draft.
  - A read while central is unreachable either fails visibly or returns the last list with an explicit "as of" time. It never presents a stale list as current.

### firing

- **R5, central keeps time** (U5).
  - The Fleet decides each due time. A Firing happens at its due time whether or not the Machine that created the Automation is online.
  - A due time that falls while an earlier Firing of the same Wake is pending folds into it. A Schedule never has more than one occupying and one waiting Run (today's rules, carried forward).
  - An Automation that's paused, disabled or ended never fires.
- **R6, firing is a message** (U6).
  - Each Firing is carried by one Delivery item, addressed to the target session's Machine, on the U63 path: central ledger, the Machine's inbox, then local delivery.
  - While that Machine is offline, the item is held and delivered when the Machine comes back, unless its "valid until" has passed. A Wake's valid-until is its end. A Schedule's is its next due time, because the next Firing supersedes it.
  - An expired item ends as **expired**, which is visible per R10.
- **R6a, edits and cancels race cleanly** (U2, U7).
  - A change, pause or cancel acknowledged before a due time decides that Firing.
  - A Firing already admitted before the change keeps the definition it was admitted with.
  - A cancel or pause reaching an admitted Firing that hasn't been delivered yet stops it through the existing message cancel. It ends as **cancelled** and is never carried out.
  - A Firing whose delivery outcome is unknown is reconciled, never fired again.
- **R7, carried out once** (U7). However many times a Delivery item is retried or redelivered, the target Machine delivers a Wake's message into its session at most once per Firing, and starts at most one Run per Schedule Firing.

### runs

- **R8, wake delivery as today** (U7). A delivered Wake Firing reaches its session as today's wake push, with the same kind and header (`PushKind::Wake`, `⏰ Router wake …`), through the same local delivery routes.
- **R9, a scheduled run as today** (U7). A delivered Schedule Firing starts one Run in the Schedule's bound session, or in a fresh session started in its folder (fresh each run). Its instruction, summary and stop rules are as today.
- **R10, outcomes are visible everywhere** (U8).
  - Each Firing's outcome is readable from every Machine: held, delivered, run started, run finished (with its summary), failed (with its reason), expired or cancelled.
  - The target Machine reports each state change back to the Fleet.
  - A later read never shows an earlier state than one already acknowledged.

### migration

- **R11, one move per machine** (U9).
  - When a Machine first runs the release that contains this design, it moves each local Schedule and each active or paused Wake to the Fleet exactly once. Each keeps its automation ID, kind, timing, next due time, target, instruction and creator.
  - An interrupted move resumes, and never makes a duplicate.
  - After the move, the Machine's local timers stop, and its local definitions are no longer used.
  - Ended Wakes aren't moved.
- **R12, no loss and no double fire across the move** (U9). Each due time that falls during the move results in exactly one Firing: either the Machine carries it out locally before handing the Automation over, or the Fleet fires it afterwards, never both and never neither.
- **R13, old machines keep working until they upgrade** (U9). A Machine still on an earlier release keeps firing its own local definitions until it upgrades and moves them. The Fleet never fires an Automation that a Machine hasn't moved yet.
- **R14, local history stays** (U10). Runs and deliveries a Machine recorded before the move stay readable on that Machine, unchanged, until the 2-month retention removes them.

### design-truth

- **R15, `main` carries the owner-decided design** (U11).
  - **What `main` must say.** After this change, `main`'s governing board-orchestration documents say what the owner decided: U38 without the wake and schedule pause, U80 and its removals throughout (requirements, specs 1–3, program designs, verification design), and no remaining contradictions.
    - The known contradictions are: PD2 "the activity gate", and `central-one-core/explainer/requirements-full.html`, which still shows the old U38.
    - Each published statement is labelled owner-settled, Lead realization, superseded, open, or planned-but-unbuilt. A docs change claims no product "done".
  - **What it must not touch.** Newer documents on `main` that the design branch lacks (the TUI, quota-dialog and Rust crate-DAG sets) stay as they are.
  - **How it's published.** Publication adds no commit that has the design branch as an ancestor. Nothing from `.cloudflare/`, `.wrangler/` or other cache directories enters the published tree or history. The same holds for secrets, account IDs or emails, real staging hosts, Access identifiers, live service, session or machine IDs, and local user paths.

## Constraints

- **C1:** the repository is public. Anything published passes the public-safety scan in R15.
- **C2:** central behaviour runs on both central runtimes, Cloudflare and self-hosted Tokio, through the one core (owner decision, 2026-10-16).
- **C3:** `interaction.sqlite` stays local on each Machine (owner, 2026-10-31).
- **C4:** every Router-owned database runs in WAL mode, with the retention and vacuum of the stacked PR below this one (owner, 2026-10-31).
- **C5:** primary keys are UUIDv7. No SQL CHECK constraints except on booleans (AGENTS.md).

## What this deliberately does not promise

- Role targets ("whoever is reviewer now"; the October design note's D5). Targets stay exact sessions or machine endpoints, as today.
- A changed rule for a fixed-target Automation whose session is recovered as a new conversation (the October note's D3). Today's behaviour stays.
- Offline creation or editing of Automations (R4).

## Proof obligations

| ID | Obligation | Passes when | Fails when | Reference |
| --- | --- | --- | --- | --- |
| V1 | R1 | An Automation created from Machine A is shown, with the same state, by `list` and `show` on Machine B; a pause on B is shown on A at the next read | B doesn't show it, or shows an older state after the write was acknowledged | R1 |
| V2 | R2 | A Firing happens at its due time while every project's lease is expired | it's delayed or paused by any lease state | U3, R2 |
| V3 | R3 | A second active Wake for one session, created from a different Machine than the first, is refused as `wakeExists`, naming the creator; each U66 limit refuses | the Fleet holds two active Wakes for one session, or a limit is accepted | U66, R3 |
| V4 | R4 | With central unreachable, `create` returns a visible "central unreachable" refusal and nothing is created later | it's accepted, queued or created afterwards | R4 |
| V5 | R5 | With the creating Machine offline, its Wake fires on time to a session on another online Machine; due times during a pending Firing fold; a Schedule never shows two waiting Runs | no Firing happens, a fold makes two, or a third Run appears | R5 |
| V6 | R6 | With the target Machine offline, a Firing is held; it's delivered once when the Machine returns; a Wake whose end passes while it's held ends as expired | it's dropped, delivered twice, or delivered after valid-until | U63, R6 |
| V7 | R7 | Forced redelivery of one Firing's item yields one session message or one Run | there are two | R7 |
| V8 | R8, R9 | The delivered Wake's push matches today's header and kind; the Schedule Run starts in the bound or fresh session with its instruction, and its summary is recorded | the header or kind differs, or the session is wrong | today's behaviour |
| V9 | R10 | Each outcome state is shown from a Machine other than the target, in order, with no regression | it's missing, out of order, or an older state is shown | R10 |
| V10 | R11, R12 | A Machine holding the owner-shaped fixture (9 Schedules, active and paused Wakes) migrates: identities equal, next due times equal; a due time inside the move fires exactly once; a killed and restarted migration makes no duplicate | an ID changes, a definition is lost or duplicated, or a due time fires twice or never | R11, R12 |
| V11 | R13 | An old-release Machine keeps firing its own definitions; the Fleet doesn't fire them until that Machine moves them | there's a double Firing, or a missed one | R13 |
| V12 | R14 | Pre-move runs and deliveries read the same after the move | they're changed or missing before retention | R14 |
| V13 | R15 | On the published branch: no governing document carries the pause clause, a lost-write obligation or an offline-created project; every statement in the decision inventory has its label; the newer `main` docs are unchanged (file manifest diff); a scan of the staged paths and the outgoing commit ancestry finds no cache, secret, account, host or live-ID material, and no design-branch ancestor | any contradiction remains, a newer doc is changed or deleted, or there's any scan hit or ancestor | U11, R15; the Advisor's 2026-10-31 inventory |
| V14 | R6a | A cancel after admission but before delivery ends as cancelled with no session message; a change before a due time governs that Firing; an unknown-outcome Firing is reconciled and not refired | it's delivered after cancel, the old definition is used, or it's fired twice | R6a |

## Coverage

| U | E | R | V |
| --- | --- | --- | --- |
| U1 | E1, E2, E3 | R1, R8, R9 | V1, V8 |
| U2 | E1, E8 | R1, R4 | V1, V4 |
| U3 | E1 | R2 | V2 |
| U4 | E3 | R3 | V3 |
| U5 | E4, E8 | R5 | V5 |
| U6 | E5 | R6 | V6 |
| U7 | E6, E9 | R6a, R7, R8, R9 | V7, V8, V14 |
| U8 | E7 | R10 | V9 |
| U9 | E1, E10 | R11, R12, R13 | V10, V11 |
| U10 | E10 | R14 | V12 |
| U11 | — | R15 | V13 |
