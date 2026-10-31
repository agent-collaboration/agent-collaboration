# Specification: central automations

Traces to [requirements.md](requirements.md): U1–U14 and U16–U18. U15 was removed. The capability keys are those defined there. Revised 2026-10-31 after the Advisor's review (F1–F11) and the owner's decisions U12–U18.

## Problem and current reality (main, 2026-10-31)

- **Definitions are per machine.** Each Router keeps its schedules and wakes in its own `automation.sqlite`, with no project and no central copy.
- **Firing is local.** That machine fires them.
- **Delivery is a local push.** A fired wake or run arrives as a Router push through that machine's `interaction.sqlite`.
- **Cross-machine wakes move the timer.** A wake for another machine is mailed to it, and that machine owns the timer (`wake_operations/remote.rs`).
- **Schedules can't cross machines.** A schedule for another machine is refused ("endpoint belongs to another service", `schedule_activation.rs:80–84`).
- **Board notifications aren't rate-limited per recipient.** Each one is its own item, and same-machine ones are immediate.
- **The published spec is stale.** It still pauses wakes and schedules on lease expiry (U3, U11).

## Entities

| ID | Term | Identity rule | Relationships | Invariants | Observable states | Basis |
| --- | --- | --- | --- | --- | --- | --- |
| E1 | **Automation** | its automation ID, minted once at creation and kept through every change and through migration | one Fleet; exactly one kind (Schedule or Wake); 0..n Firings | no project (U3); one definition, one place (U2) | per kind | U1–U3 |
| E2 | **Schedule** | an Automation of kind schedule | has one timing (at, interval or cron with its time zone; R6g) and one instruction (E11). It also has today's execution settings: model, effort, timeout, and a destination, either an existing session or a machine endpoint plus folder (fresh each run). Its continuity comes from its previous completed Run on its destination machine | at most one occupying and one waiting Run; later due times fold into the waiting Run, which never expires (today's rule) | enabled, disabled, deleted; **suspended** only after a move with an inexpressible timing (R11) | U1, U7 |
| E3 | **Wake** | an Automation of kind wake | has one target session, one due time, an optional repeat, one end (expiry) and one creator | the wake limits (R3); the target session's wake slot is held by an active **or paused** Wake | active, paused, ended (expired, cancelled or completed); **suspended** only after a move with an inexpressible timing (R11) | U1, U4, U13 |
| E4 | **Firing** | one Automation plus one **due time**, always a grid time (E10) | belongs to one Automation; is an entry in exactly one Batch (E5) once sent; yields at most one wake delivery or one Run | carried out at most once (R7); cancelable only before the send cutoff (R6a) | the R10 states | U5–U8, U17 |
| E5 | **Batch** | its own batch ID, minted when central freezes it; each chunk of a split due set is its own Batch with its own ID | addressed to exactly one Machine; carries 1..n Machine work item entries (E14) of two kinds: a **Firing** entry (target, revision, expiry, **content references**, i.e. the revision ID plus digest, never the text), or a **SessionCreate** entry (the frozen full request) | bounded entries (R5); the same bytes on every resend | frozen, notified, fetched | U6, U18 |
| E6 | **Run** | one Schedule Firing carried out on its destination machine | belongs to one Firing; runs in exactly one session | one per Firing | today's run states | U7 |
| E7 | **Firing outcome** | the latest observable state of one Firing | readable from any Machine | never moves back to an earlier state (R10) | as E4 | U8 |
| E8 | **Fleet** | the organization's central (one organization today) | holds the one list | — | — | U2, Boundary |
| E9 | **Machine** | an enrolled Router installation | receives Batches; hosts sessions | carries out what it receives | online or offline | U6, U7 |
| E10 | **Grid time** | an instant on a 5-minute boundary of UTC time | every due time an Automation can produce is one | an off-grid time is refused at entry, never rounded | — | U12 |
| E11 | **Instruction** | its instruction ID, with numbered revisions (today's meaning) | used by 0..n Schedules | a Firing freezes the revision in force at admission | — | U1, U9 |
| E12 | **Notification** | one board notification, thread-activity notice, or subscription-expiry notice item. **Not** a direct message, wake, schedule run, approval or question | addressed to one recipient session | — | today's item states | U14 |
| E14 | **Machine work item** | its work item ID, minted once by central when it admits the request (a Firing's entry, or a remote session creation) | addressed to exactly one Machine; carried in a Batch (E5) | the same custody, acknowledgement and report rules as Firing entries (R6, R7, R10) | pending, held, received, then the kind's own outcome | U6, U20 |
| E13 | **Limit** | one named product limit, with its value and step | lives in the published contract | the same value in the schema, the validators, and the CLI and MCP descriptions | — | U16 |

## Obligations

### definitions

- **R1, one list everywhere** (U2). From any Machine, the CLI and MCP create, list, show, change, pause, resume and cancel Automations. Once a write is acknowledged, any later read from any Machine shows it, and there's no per-machine list.
- **R2, no project** (U3). An Automation carries no project. A lease state never stops, delays or pauses a Firing.
- **R3, the wake rules hold everywhere** (U4, U13).
  - **Timing:** a one-shot Wake is due at most 24 h ahead; a repeating Wake ends within 7 days; a repeat is at least **10 minutes**.
  - **The slot:** a session's wake slot is held by an active or paused Wake. Setting another needs `replace: true`, and only the existing Wake's creator, the target session or the owner may replace it; anyone else is refused as `wakeExists`, naming the creator. A Wake with no recorded creator can be replaced only by its target session or the owner.
  - **Replacement is atomic:** the old Wake ends as cancelled and the new one becomes active in one step. A replayed replacement returns the same result.
- **R4, writes need central, and retries are safe** (U2, U16).
  - Every create, change, pause, resume or cancel carries the caller's **operation ID**. Repeating the same operation with the same content returns the original result; the same ID with different content is refused as `operationConflict`.
  - Central unreachable **before sending** gives a definite `centralUnreachable`, and nothing is queued.
  - A request sent but **unanswered** gives `outcomeUnknown` with its operation ID. Retrying it with that ID returns the real result. It's never reported as not applied.
- **R5, the limits are the contract** (U16). Every limit has one definition: `GRID_STEP` = 5 min, `WAKE_MIN_PERIOD` = 10 min, `WAKE_ONE_SHOT_MAX_AHEAD` = 24 h, `WAKE_REPEAT_MAX_LIFETIME` = 7 d, `SCHEDULE_MIN_INTERVAL` = 5 min, `NOTIFICATION_INTERVAL` = 10 min, `NOTIFICATION_LISTING_MAX_ENTRIES` = 50, `BATCH_MAX_ENTRIES` = 100, `REPORT_FLUSH_MAX` = 100 reports, `REPORT_FLUSH_WINDOW` = 500 ms (owner), instruction text ≤ 1 MiB (today's `instruction_document.rs:47`), wake message ≤ 1 MiB (today's `MAX_CONTROL_FRAME_BYTES`), plus today's identity field limits (`identity_schemas.rs`), and the slot rules (R3). That definition appears in the published schema (minimum, maximum, step, description). Every entry (CLI, MCP, central calls) and every decode of a stored row checks it. A violation is refused as `limitExceeded` with the limit's name, the value and the allowed range. The CLI help and MCP tool schemas show the same values.

### cadence

- **R6g, the 5-minute grid, with 5-minute precision** (U12). Every due time an Automation can produce is a grid time (E10). Entry refuses anything else as `offGrid`, naming the nearest earlier and later grid times; nothing is rounded.
  - **at:** the time must be a grid time.
  - **interval and repeating Wakes:** the period is a multiple of 5 minutes (at least 5 for a Schedule, 10 for a Wake), and the first due time is a grid time, given by the caller or, when omitted, the next grid time, which the answer states.
  - **cron:** every minute the expression can produce is a multiple of 5, checked over its whole expansion in its time zone.
  - **after:** relative timing is not accepted for Automations; the refusal names the grid times it would fall between.
  - Time zones and DST apply when computing a due time; a zone whose offset isn't a multiple of 5 minutes is refused.
  - The end of a repeating Wake is not a firing time and need not be on the grid.
  - The one place times are fitted is the move of existing definitions (R11), because that data already exists.
- **R6n, notifications at most once every 10 minutes per recipient** (U14).
  - **Rate:** a recipient session receives at most one Notification delivery in any 10 minutes.
    - A Notification arriving 10 minutes or more after that recipient's last Notification delivery is delivered at once.
    - Otherwise it's held, and everything held is delivered **together** 10 minutes after the last delivery, as one listing.
    - The last-delivery time survives restarts.
  - **Listing size:** a listing holds at most a fixed number of entries (a Limit). Beyond that, it shows the count and a pointer to read the rest. No accepted item is lost; each stays in the recipient's inbox and feed.
  - **What stays immediate:** direct messages, wakes, schedule runs, approvals and questions.
  - **What's unchanged:** the replication gate for remote notifications (U63), and the recipient's Off and Poll policies.
  - **What's superseded:** U63's "same-machine delivery stays immediate" for notifications only.

### firing

- **R7f, central fires at the due time, batched per machine** (U5, U18). At each due time, central admits every due Firing once. The Firings for the same Machine go to it as one Batch, sent right away. Nothing is held on the Machine for later release. A second alarm for the same due time admits nothing new and resends the same Batch bytes. **Central's clock never stays stopped:** a permanent error in one Automation (an invalid definition) is reported on it and never stops the others; a temporary failure leaves every owed Firing owed; and if the platform alarm is lost after its retries run out, the next central request re-arms it for the earliest pending due time, with no due Firing lost.
- **R6, firing is a message** (U6).
  - **Custody, the U63 contract applied to automations.**
    - **Central keeps the Batch.** A frozen Batch stays in central's custody until its Machine **acknowledges** it, after the Machine has durably admitted every entry locally. A Schedule entry never expires there.
    - **The Machine can always find it.** It asks central for its unacknowledged Batches whenever it connects, reconnects, gets a hint, or finds that its notice position has fallen past the retained window. A notice is only a wake-up hint. It's never the only way to find owed work.
    - **Few, larger messages.** A Machine pulls all its pending Batches, fetches missing content, acknowledges, and reports in as few calls as possible: one pull returns many Batches, one acknowledgement covers many, and reports are flushed together, at most every `REPORT_FLUSH_WINDOW` or `REPORT_FLUSH_MAX` reports, whichever comes first. This follows Cloudflare's WebSocket guidance to batch logical messages into fewer frames.
    - **Repeats are safe.** A lost fetch or acknowledgement is simply repeated. A re-fetch after a crash re-admits nothing already admitted (R7).
    - **Content stays available** while any unacknowledged Batch or live definition references it.
  - **What "the same message path" means here** (U6, a Lead realization of the owner's words): central holds the item until the receiver has it durably, and the receiver delivers it into the session through the same local delivery routes as every Router message. Session-to-session messages keep their mailbox custody unchanged.
  - **Expiry is per entry:** a Wake entry carries its Wake's end, a Schedule entry carries none. While the Machine is offline the Batch is held. When it returns, each Wake entry past its end ends as **expired**, and every other entry is carried out.
  - **Size:** a Batch holds at most `BATCH_MAX_ENTRIES` entries. Larger due sets are split into several Batches at admission, each with its own batch ID. Content travels by reference and is fetched once per revision, so a 1 MiB instruction never has to fit inside a Batch.
  - **Old releases:** a Machine whose Router can't receive Batches can't be a target, and creating an Automation for it is refused as `targetNeedsUpgrade`.
- **R6a, the send cutoff** (U17).
  - A Firing is **pending** until the central commit that freezes it into a Batch; that commit is the cutoff.
  - A pause, cancel or change acknowledged before the cutoff removes or updates the Firing.
  - After the cutoff, the Firing is out. The command's answer says `alreadySent` for it, and nothing is sent to the Machine to chase it.
  - If central can't tell whether the notice was appended, the Firing shows **unknown** until central reconciles it against the machine's notice position. It's never announced again under a new identity.
- **R7, carried out once** (U7). However many times a Batch is delivered, each entry results in at most one wake delivery or one Run. A crash at any point resumes the entry's unfinished work; it never drops it or repeats completed work.

### runs

- **R8, wake delivery as today** (U7). A Wake entry reaches its session as today's wake push (`PushKind::Wake`, `⏰ Router wake …`), through today's routes and modes.
- **R9, a scheduled run as today** (U7). A Schedule entry starts one Run in the bound session, or in a fresh session in its folder. It runs with the Instruction revision frozen at admission and the Schedule's model, effort and timeout. Its continuity comes from the previous completed Run on that Machine. The run is driven through preparation, execution, stop and summary as today.
- **R10, outcomes are visible everywhere** (U8, U17). Each Firing's outcome is readable from every Machine. The states are:
  - **pending:** not yet frozen;
  - **cancelled:** stopped in central before the cutoff (U17);
  - **held:** frozen and announced, not yet fetched; the Machine is offline or hasn't caught up;
  - **received:** fetched by the Machine;
  - **delivered:** a Wake reached its session;
  - **run started**, then **run finished** (with summary) or **failed** (with reason), for a Schedule;
  - **expired:** a Wake's end passed before delivery;
  - **uncertain:** the Machine can't tell whether the session accepted the input. Reconciled by the Machine's existing rules to delivered, run started or failed, and never repeated;
  - **unknown:** central can't tell whether the notice was appended; reconciled as R6a says.

  **Ordering:** each report carries a per-Firing sequence number stored durably on the Machine with the state change, so a restart continues the sequence. Central applies reports in that order and ignores duplicates and older ones. Terminal states (cancelled, delivered, run finished, failed, expired) are final.

### migration

- **R11, one move per machine** (U9).
  - On first start of the new release, the Machine moves each local Schedule, each active or paused Wake, and the Instructions they use to the Fleet, exactly once. IDs, targets, creators and execution settings are kept.
  - **Timing is normalized under U12 and U13** (the owner's later decisions qualify U9's "keeps timing"):
    - **each definition is converted once into a timing R6g accepts**, so nothing is fitted after the move;
    - a one-shot (`at`, or `after` whose due time is already computed) becomes `at` the next grid time at or after its due time;
    - an interval Schedule or repeating Wake keeps its period if that is a multiple of 5 minutes and at least its minimum (5 minutes for a Schedule, 10 for a Wake); otherwise the period rises to the next one that is (7 → 10, 11 → 15, 60 seconds → 5 minutes). Its first due time becomes the next grid time at or after its next local due time;
    - a cron expression is rewritten with every minute it can produce raised to the next multiple of 5, as a literal minute list (`9 * * * *` → `10 * * * *`). A minute of 56–59 becomes minute 0 of the next hour: with an hour field of `*` that's the same expression at minute 0; with literal hours, each affected hour moves up by one, and hour 23 wraps to 0 only when the day, month and weekday fields are all `*`;
    - **Migration policy (owner assent 2026-10-31: "yes its fine"):** a cron that still can't be expressed after that (hour 23 wrapping into a constrained day) moves as a **suspended** Automation. Its original expression is kept verbatim as `legacy_timing`, and it has **no executable timing**. Entry and decode validate suspended Automations as that state, never running R6g over `legacy_timing` and never coercing it. It fires nowhere, and the handover report and an `automation needsTimingFix` event name it until someone edits it to a grid timing. None exists in the owner's data;
    - an end time is never extended, and a Wake whose next fitted due time would pass its end ends as **expired**, recorded with an event.
  - **Conversion follows the R12 freeze:** the watermark records original local times before anything is converted; a carried waiting Run's due time is converted like a one-shot; an external occupant is carried unchanged. The converted definition is what the frozen payload holds, so every retry sends the same bytes.
  - The handover reports how many definitions were adjusted or ended.
- **R12, one owner at every moment** (U9).
  - In one local transaction, the Machine:
    - stops admitting new local due times for the Automation;
    - records its timer watermark: the last **original local** due time evaluated (before fitting), including times folded into a waiting Run;
    - cancels a not-yet-started waiting Run, carrying that Run's due time into the payload as central's waiting slot;
    - freezes and stores the payload.
  - **A local Run that may still be executing stays occupying the schedule.** That covers preparing, executing or uncertain. The payload names it as an external occupant.
    - Central sends no Run for that Schedule until the Machine reports the external Run settled.
    - The report comes through the existing reconciliation; uncertain is never treated as finished.
    - A pending local wake delivery is carried the same way, as a pending Firing that later due times fold into.
  - **The watermark is applied once, at conversion.** While freezing, the Machine drops every occurrence whose original local due time is at or before the watermark, then converts what remains and computes the first central grid due time and its `admitted_through_ms`. Those go into the frozen payload. After import, central generates only from the converted definition; nothing keeps an original-time generator or fits times at runtime.
  - Every retry sends the same frozen bytes. An interrupted move resumes, and never makes a duplicate or leaves two owners of admission.
- **R13, old machines keep working until they upgrade** (U9). A Machine on an earlier release keeps firing its own definitions; the Fleet never fires an Automation it hasn't received.
- **R14, local history stays** (U10). Pre-move runs and deliveries stay readable on their Machine until the 2-month retention removes them.

### remote-sessions

- **R16, create a session on another machine** (U20).
  - **The request.** A `conversation create` whose endpoint belongs to another enrolled Machine goes to central with the caller's operation ID. It carries:
    - the **creator**: the requesting person (the human user at machine A) or session on machine A;
    - the **approver** for the new session's permission prompts;
    - the endpoint, model, effort, access, folder, and an optional first input.
  - **The creator is checked at the source.** Central checks that the creator belongs to the Machine that sent the request (the human user at that installation, or one of its sessions), and refuses a mismatch as `originMismatch`.
  - **Codex creators are sessions.** On a Codex endpoint the creator must be a session, as today (`codex_create_actors`); a human requester creating on a Codex endpoint is refused `creatorNotSupported` before any effect, never mapped to an invented session. Provider endpoints accept either.
  - **The approver is checked at the target.** The approver must be allowed by the target Machine's policy for that endpoint. Codex endpoints accept only a session approver, which must be a session on the target; provider endpoints also accept the owner. Approvers on another Machine are refused as `approverNotSupported`, because cross-machine approvals were deferred in board design U63. **Default approver (interim, owner 2026-10-31):** when the request names none, the target Machine's named Chief of Staff session approves. A Machine with none named refuses the create as `approverRequired` before any effect. The interim ends when routed approvals land (a separate later design, where the default becomes the requester).
  - **Delivery.** It's sent at once, not on the grid, as a Machine work item under the same custody as Firings: held until the target acknowledges, nothing missed, and repeats safe.
  - **The target decides before any effect.** Using its existing local checks (the endpoint and its advertised models, the access level, the folder), it refuses with a typed reason before creating anything. There's no new allowlist.
  - **What's created.** The session records its creator as that requester on the source Machine, and its approver as given. The target's approval route accepts that foreign creator **only** for a create admitted from a central work item, carrying central's verified creator provenance; the approver must still be on the target. Every other route registration keeps today's same-Machine check for both fields.
  - **The answer has two parts.** The **session**: created (with its address; settings applied, or partly applied with details), refused (with the target's reason), or unknown. The **first input**: none, accepted (the new session has received it), failed (with reason), or unknown.
  - **What it never does.** A created session is never hidden, rolled back or created twice. A first input is never repeated automatically.
  - **Waiting.** The caller waits up to its timeout for both parts, then returns **pending** plus the operation ID. Stopping the wait cancels nothing; `operation show` and `wait` read the same result later.
  - **Retries.** The same operation ID with the same content returns the same result; with different content it's `operationConflict`.
  - **Old releases.** A target that can't receive Machine work items is refused at the request (`targetNeedsUpgrade`).

### design-truth

- **R15, `main` carries the owner-decided design** (U11).
  - `main`'s governing board-orchestration documents say what the owner decided: U38 without the pause, U80 throughout, U14's notification cadence noted against U63, and no remaining contradictions (PD2's "activity gate", and the `requirements-full.html` explainer).
  - Each statement is labelled owner-settled, Lead realization, superseded, open or planned-but-unbuilt.
  - Newer `main` documents stay unchanged.
  - There's no commit with the design branch as an ancestor, and no cache, secret, account, email, host, live ID or local-path material.

## Constraints

- **C1:** the repository is public.
- **C2:** the one core runs on both central runtimes.
- **C3:** `interaction.sqlite` stays local.
- **C4:** every Router database runs in WAL mode under PR 1's retention and vacuum.
- **C5:** primary keys are UUIDv7; there are no SQL CHECK constraints except on booleans.
- **C6:** there is one organization today.

## What this deliberately does not promise

- role targets (the October D5);
- a new rule for sessions recovered as new conversations (D3);
- offline creation;
- chasing a sent Firing (U17);
- holding Firings on a Machine for later release (B, closed);
- organization offsets (U15, removed).

## Proof obligations

| ID | Obligation | Passes when | Fails when |
| --- | --- | --- | --- |
| V1 | R1 | A write on A is read identically on B; a pause on B shows on A | a per-machine list, or a stale read after an acknowledged write |
| V2 | R2 | A Firing happens at its due time with every lease expired | any lease gate |
| V3 | R3 | A paused W1 blocks W2; an authorized replacement ends W1 and starts W2 once; a foreign replacement is refused naming the creator; each timing limit refuses; a replay returns the same result | two Wakes in one slot; a limit accepted |
| V4 | R4 | Drop the reply after commit: a retry with the same operation ID returns the original; changed content conflicts; pre-send unreachable is definite and nothing is created | a second definition; `centralUnreachable` after an applied write |
| V5 | R5 | For each Limit, the schema's bound equals what's accepted at the CLI, MCP and central, and a corrupted stored row is refused on decode with `limitExceeded` | a mismatch, or silent coercion |
| V6 | R6g | Fixtures for at, interval, cron (with time zone and DST), repeating Wakes and after: grid times are accepted and fire at exactly that time; every off-grid input (at 10:02, a 7-minute period, cron minute 3, any after) is refused `offGrid` naming the two nearest grid times; an omitted first due time is the next grid time and is stated | an off-grid time accepted, rounded or fired at another time; a sub-5 interval accepted |
| V7 | R6n | 30 Notifications across 3 projects and 3 kinds to one recipient within 10 minutes give one immediate delivery, then one listing; a Notification arriving just after that listing is held until 10 minutes after it, including across a restart; a direct message, wake, approval or question in the same window arrives immediately; an overflowing listing points to the rest with no item lost; Off and Poll are respected; a remote notification still waits for replication | more than one delivery per window, a delayed direct message, or a lost item |
| V8 | R7f | The alarm handler fails on one Automation: the others still fire and the alarm re-arms. The alarm is lost after exhausted retries: the next create, list or machine pull re-arms it, and every overdue Firing is sent once. From an empty Fleet journal, creating one Automation and restarting central fires it at its time with no further call. Five Firings due at one grid point for two machines give two Batches. A duplicate alarm resends the same bytes and admits nothing new. 250 due entries split into three Batches with distinct IDs. Unrelated Fleet notice work isn't delayed | a Firing admitted twice, or different bytes on a resend |
| V9 | R6 | A mixed Batch (a Wake with an end plus a Schedule) for a Machine offline for 8 days, past the notice window: on reconnect (notice position expired) it lists, fetches, admits and acknowledges. The Schedule runs once, the Wake is expired. A lost fetch reply, a lost acknowledgement, and a crash before and after local admission each recover exactly once. A target on an old release is refused | the Schedule dropped, the Wake delivered after its end, or an old target accepted |
| V10 | R6a | Pause before the cutoff: the Firing isn't sent. Pause after: the answer says `alreadySent` and the same Batch is delivered. A lost send answer reconciles to sent, or shows unknown, with no second identity | a false prevention, a chase message, or a duplicate |
| V11 | R7 | Crash after receiving a Batch, before any entry's work; after two of three entries; after native acceptance, before the outcome is stored. Each restart gives exactly one effect per entry | an entry lost or repeated |
| V12 | R8, R9 | The wake push header, kind and mode behaviour are today's. On a B that never held A's definitions: a Run starts, uses the frozen Instruction revision even after a later edit, keeps continuity across two fresh runs, and finishes its summary after a restart of B mid-run | a wrong revision, missing continuity, or a stuck summary |
| V13 | R10, R6 (U19) | B offline shows held; a cancel before the cutoff shows cancelled; native acceptance with its answer lost shows uncertain, then reconciles; reports reordered and duplicated, and the reporter restarted, keep the sequence | a regression; a reset sequence; a missing held or uncertain state |
| V14 | R11, R12 | The owner-shaped fixture (9 Schedules including a 60-second interval, Wakes with 5-minute repeats and near-end expiries) moves with IDs kept; timing is normalized per R11; adjustments are reported. A 7-minute interval becomes 10, an 11-minute Wake repeat 15, `9 * * * *` becomes `10 * * * *`, an `after` one-shot becomes `at` its fitted grid time, a cron at minute 58 with an hour field of `*` becomes minute 0, and a cron that can't be expressed arrives **suspended** with its `legacy_timing` intact and fires nowhere. Every converted **executable** timing passes R6g entry validation, and the suspended one passes decode as suspended. The 10:02 one-shot handled before a 10:03 upgrade is checked on the imported definition itself: its stored first `due_ms` and `admitted_through_ms` exclude it. Several later due times of each converted definition are on the grid; a restarted or retried import produces the same converted bytes; a carried waiting Run keeps its place at its fitted grid time; and no occurrence handled locally before the watermark is admitted again. A one-shot due at 10:02, admitted locally before an upgrade at 10:03, runs once and never again at 10:05. A fresh-each-run R0 accepted natively with its answer lost (uncertain) at handover: no successor run starts until R0 is reconciled and reported settled. A not-yet-started waiting run becomes central's waiting slot and runs once. Kill the move before and after the local cutoff, the central commit, and the local acknowledgement, with admitted, unsubmitted and uncertain local work outstanding: each due time ends with one effect | an ID changed, an end extended, two owners, or a double or missing effect |
| V15 | R13 | An old-release Machine fires locally; the Fleet has none of its rows | a double Firing |
| V16 | R14 | Pre-move history reads are identical | missing before retention |
| V18 | R16 | From A, create a session on B with a first input. A gets B's address, the creator shows A's requester, and with no approver named the approver is B's named Chief of Staff, and a permission prompt raised in the new session reaches that Chief, whose decision resumes the session (a real request, decision and callback); with none named on B the create is refused `approverRequired` before any effect. A spoofed creator from another Machine is refused (`originMismatch`); a session approver on A is refused (`approverNotSupported`), and a human approver on a Codex endpoint is refused; B's own access, folder and model refusals come before any effect. With B offline the result is pending, and B creates it exactly once on return. Kills before and after local admission and acknowledgement, after native creation before its report, and after first-input acceptance before its result: the same operation returns the same session, with no second create and no second input. Settings applied only partly are shown. A first input that fails after creation shows the session as created and the input as failed. A caller timeout returns pending, and the work continues | a second session or input; a hidden created session; a lost request; a misattributed creator; the old 'belongs to another service' refusal |
| V17 | R15 | The manifest leaves newer docs untouched; no contradiction remains; the scan of the tree and the outgoing ancestry is clean | any hit |

## Coverage

| U | R | V |
| --- | --- | --- |
| U1 | R1, R8, R9 | V1, V12 |
| U2 | R1, R4 | V1, V4 |
| U3 | R2 | V2 |
| U4 | R3 | V3 |
| U5 | R7f | V8 |
| U6 | R6 | V9 |
| U7 | R7, R8, R9 | V11, V12 |
| U8 | R10 | V13 |
| U9 | R11, R12, R13 | V14, V15 |
| U10 | R14 | V16 |
| U11 | R15 | V17 |
| U12 | R6g | V6 |
| U13 | R3 | V3 |
| U14 | R6n | V7 |
| U16 | R5 | V5 |
| U17 | R6a | V10 |
| U18 | R7f, R6 | V8, V9 |
| U19 | R6, R5 | V13, V5 |
| U20 | R16 | V18 |
