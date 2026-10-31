# Requirements: central automations

This set fixes a mental-model break. The owner's model of automations was never built:

- **The model:** automations are definitions of scheduled messages, kept in one central place for every machine.
- **What's on `main` (2026-10-31):** each machine keeps its own schedules and wakes in its own `automation.sqlite` and fires them itself. Only a wake aimed at a session on another machine reaches central, as mail.

This set also covers the design truth that never reached `main`. The board-orchestration design, including the owner's 2026-10-19 decision that automation is never tied to a project, exists only on a local branch.

It stacks on `fix/router-db-retention`: database retention, vacuum and WAL, decided by the owner on 2026-10-31. The owner merges both PRs together: "both 2 prs in the stack can go in together".

## Who is affected

- **The owner,** who sets schedules and wakes for agents and expects them to work across machines.
- **Agents** (Leads, Sidekicks, Workers), which set wakes and schedules for themselves and for each other, and receive them.
- **Every machine's Router,** which today fires automations itself.
- **Central** (the Cloudflare and self-hosted Tokio runtimes, one core), which today holds mail only while it's in transit.

## Capabilities

- `definitions`: **Automation definitions.** Automations are defined once, in one central place, and look the same from every machine.
  - U1, U2, U3, U4
- `firing`: **Firing.** A due automation becomes a message, routed to its target session wherever it lives.
  - U5, U6, U17, U18
- `runs`: **Runs.** The machine that receives a fired automation carries it out, and its outcome can be seen from anywhere.
  - U7, U8
- `migration`: **Migration.** Today's per-machine schedules and wakes move to central once, with nothing lost and nothing fired twice.
  - U9, U10
- `cadence`: **Cadence.** Automations run on a shared grid, and notifications reach each recipient at a bounded rate, so cost and load stay predictable for a service.
  - U12, U13, U14, U16
- `design-truth`: **Published design truth.** The design on `main` says what the owner decided.
  - U11

## Rows

Authority:
- **authorized:** the owner's own words.
- **derived:** follows directly from authorized rows, and the derivation is named.

| ID | Capability | Need or outcome | Why it matters | Evidence and authority | Priority |
| --- | --- | --- | --- | --- | --- |
| U1 | definitions | An **automation** is the definition of a scheduled message: **when** (a schedule's timing, or a wake's time and repeat), **to whom** (an existing session, or a fresh session started on a machine in a folder), and **what** (the message or instruction it delivers). Schedules and wakes are both automations. | The owner's model, stated plainly; every other row is about this thing. | Owner, 2026-10-31: "automations are basically definitions of message schedules. routing messages." **authorized** | P1 |
| U2 | definitions | Automation definitions live **in one central place shared by every machine**. From any machine, through the Router CLI or MCP, an authorized caller can create, list, show, change, pause, resume and cancel any automation, and sees the same list and the same state. | Today an automation exists only on the machine that made it, and nothing about it is visible elsewhere. | Owner, 2026-10-31: "it was supposed to be cross system in a central location." Owner, 2026-10-19, asking the Advisor: "did we not put wakes and automations in central not in a project?" (native rollout 01a140ac…, line 30505). **authorized** | P1 |
| U3 | definitions | An automation is **not tied to any project**. It runs whatever any project's lease state is. | The published spec on `main` still says a project's wakes and schedules pause on lease expiry. The owner removed that rule, but the removal never reached `main`. | Owner, 2026-10-19: "ok then automation is associated or should not be associated with anything should always work" (Claude transcript 9f6bdec4…, line 72299). Board design U38 as rewritten, and U80 (`b8e63fedf`, on the local `design/board-orchestration` only). **authorized** | P1 |
| U4 | definitions | **The wake limits stay as decided, except the minimum period, which U13 raises to 10 minutes.** Wakes are bounded: one-shot at most 24 h ahead, and repeating wakes end within 7 days. A session has at most one active wake. Each wake records who set it. Long-lived timing is a schedule. | Moving definitions must not quietly change their rules. | Board design U66 and MS-10. Owner, 2026-10-15: "all wakes should have a time limit (different from schedule)"; "for now its fine the wakes the way they are and with a max period and max time. the rest is schedules". **authorized** | P1 |
| U5 | firing | **Central keeps the time and decides when an automation is due.** Firing doesn't depend on the machine that created the automation being online. | A definition kept centrally but fired by one machine would still stop when that machine is off. | Derived from U2 ("central location") and U3 ("should always work"). This settles the timer-location decision left open since 2026-10-04: the board Lead proposed "DEFINITION (what, when, to whom) ── global: on the fabric", then "automation.sqlite (one owner, one clock)", and asked the owner to choose between the fabric and a per-project timer home (D2 / ADR-9, PROPOSED in #124 `21a9b029f`). The owner never closed it. The owner closed it on 2026-10-31: when told that central holds the definition and the clock and pushes the mail (Claude session 5010c475, line 2469), the owner agreed that central can push the automation mail (line 2472), then chose "A with send time batching" (U18). Claude transcript cacb0963…, lines 7825, 7917, 7943. **derived** | P1 |
| U6 | firing | **A fired automation travels as a message on the same path as every other message:** central's ledger, then the target machine's inbox, then local delivery. Nothing is missed. While the target machine is offline, the item is held and delivered when the machine returns, unless it carries a "valid until" that has passed. | The owner made schedules and wakes use the message path, and made that path reliable across machines. | Owner, 2026-09-27: all agents message each other through the Router, and "schedules, wakes and board listens push through this same message path, so it must actually work". Board design U63 (owner, 2026-10-15): outbox, central ledger, inbox; "Nothing is missed"; "Undelivered items are held until delivered". Owner, 2026-10-31: "automations … routing messages". **authorized** | P1 |
| U17 | firing | **A pause, cancel or change affects only firings not yet sent.** Once a firing has left central, it's out: nothing chases it to the machine. Its outcome then says it was already sent when the pause, cancel or change arrived. | Simple and honest: no cancel race on the machine, and no false "cancelled". | Owner, 2026-10-31: "we do not have to chase to the machine for cancel". **authorized** | P1 |
| U18 | firing | **Central fires at the due time, and batches at send time.** At each grid point, every firing due for the same machine goes to it as **one** message, sent right away. Nothing is held on a machine for later release. | It roughly halves the per-firing cost of the service model with none of the hold, cancel or amendment machinery of planning ahead. | Owner, 2026-10-31, choosing between the options on `firing-options.html`: "A with send time batching". **authorized** | P1 |
| U7 | runs | **The receiving machine carries out a fired automation as today.** A wake or message is delivered into its session. A scheduled run uses its bound session, or starts a fresh session in its folder when the schedule says fresh-each-run. Each firing is carried out once, never twice. | The move to central changes who keeps time, not what a run does. | Owner, 2026-09-27: "it must actually work, not work best-effort". Today's behaviour is in the destination kinds `OwnedThread` and `FreshEachRun` (`agent-automation`). **authorized** (carried forward) | P1 |
| U8 | runs | **The outcome of every firing can be seen from any machine** that lists the automation: held, delivered, run started, run finished (with its summary), failed, expired or cancelled. | With definitions in one place, their results must be too, or "what did my schedule do?" depends on which machine you ask. | Derived from U2 (the same list and state everywhere) and U6 ("an outcome the sender can see"). **derived** | P1 |
| U9 | migration | **Each machine's existing schedules and active wakes move to central once.** They keep their identity, timing, target, instruction and creator. During the move, none is lost and none fires twice. Afterwards no machine keeps its own definitions or timers. | The owner has nine schedules and live wakes today; a move that drops or doubles them breaks agents' work. | Owner, 2026-10-31: "we need migrations and a bunch of changes". **authorized** | P1 |
| U10 | migration | **Run and delivery history already recorded on a machine stays readable there** until the retention window removes it: 2 months, under the stacked database PR. | Recent history is how the owner checks what happened. | Owner, 2026-10-31: retention table ("everything two months is fine for automation"). **authorized** | P2 |
| U12 | cadence | **Every schedule and wake runs on a 5-minute grid.** Due times are rounded to 5-minute boundaries, so everything due in an organization at one grid point is handled together. | Shared due times let central wake once and send one batch per machine; this is the main cost lever for a service. | Owner, 2026-10-31: "we centralize on yes i think all schedules can be rounded to 5 with wakes and schedule"; "it's not about my peak if i want this to be a service". **authorized** | P1 |
| U13 | cadence | **Wakes fire at most every 10 minutes.** This replaces U66's 5-minute minimum period, and the other U66 limits stay. | Bounds the most frequent automation and its cost. | Owner, 2026-10-31: "wakes should be 10min min on boards". Scope: the Lead read this as all wakes and told the owner so ("If you meant only wakes created by board activity, tell me"); the owner didn't narrow it. **authorized** (the scope reading is the Lead's, and the owner may still narrow it) | P1 |
| U14 | cadence | **Notifications, board ones and all others, reach a recipient at most once every 10 minutes.** Everything arriving within the window is delivered together, as one delivery listing each item. Direct messages between agents are not notifications and stay immediate. | Notification storms cost requests and interrupt agents; a bounded rate keeps both predictable. | Owner, 2026-10-31: "i think we can have board notifications and all notifications every 10min minimum". **authorized** | P1 |
| U16 | cadence | **The limits are part of the product contract.** The grid, the wake minimum period and lifetime, the notification interval, the wake slot and replacement rules, and field sizes:<br>• are written into the published schema, with minimum, maximum and step values and a plain description;<br>• are validated at every entry (CLI, MCP, central calls) and when stored rows are read back, by typed Rust constructors. A refusal names the limit, with no SQL CHECK constraints (AGENTS.md);<br>• appear in the MCP tool schemas and CLI help, so an agent knows the rules before it calls. | Agents work from the contract; a limit that only exists deep in code gets violated, then refused, then retried. | Owner, 2026-10-31: "the product contracts should be part of schema and max validation too and understood by agents". **authorized** | P1 |
| U11 | design-truth | **The design published on `main` says what the owner decided.** The board-orchestration design that exists only on the local `design/board-orchestration` (667 commits ahead of `main` on 2026-10-31, including U80) is published to `main`, without cache or secret material. Obligations the owner removed, such as the automation pause in U38, are gone from `main`. | Helpers read `main`; while the decisions are missing there, work is built against superseded design. That's how this break went unseen. | Owner, 2026-10-31: "we need to fix it". Root cause: `b8e63fedf` is on no remote branch. #160 (`be7e61e8b`, 2026-10-25) published the requirements blob `8695eec4…`, which is exactly `b8e63fedf`'s parent, not the corrected `c2072d35…`. On 2026-10-22 the importer's reviewer had already noticed that the design documents in the single tree were "an older copy" (Claude transcript 9f6bdec4…, line 111853), and that was never repaired (Advisor, 2026-10-31). **authorized** | P1 |

## Boundary

**In scope:**
- automation definitions, their central home and timing;
- firing as messages on the U63 path;
- running on the receiving machine;
- outcome visibility;
- migration of existing definitions;
- the CLI and MCP surface for all of the above;
- publishing the board-orchestration design to `main`.

**Out of scope:**
- **Interactions storage.** `interaction.sqlite` stays local on each machine (owner, 2026-10-31: "interactions.sqlite should be local").
- **Project binding of automations** (U3).
- **Changes to the wake limits** (U4).
- **Database retention, vacuum and WAL.** These are the stacked PR below this one. This set relies on them and doesn't change them.
- **A terminal UI for automations.** Nobody asked for it.
- **Multiple organizations.** There is one organization and one central today; a multi-organization service is a separate project (owner, 2026-10-31: "its a separate project rn, we just have one org"). This set targets the one central. The service-scale cost model on `firing-options.html` is context, not scope.

## Unresolved hypotheses

- **H1, missed firings while offline.** For a repeating schedule whose target machine is offline across several due times, U6 says items are held. This set carries forward today's rule for how many held runs a single schedule may accumulate (its overlap policy) rather than inventing a new one. The Specification states that rule from today's behaviour.
