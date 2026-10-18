# V6 replay cases

Three cases extracted from real codex-router work for spec 1 V6: replay each case's events through a new project board, then check that the board's views answer "what was decided", "where did it stop" and "what proves it".

| File | Case | Events | Probes |
|---|---|---|---|
| `messaging-first-112.json` | #112 Messaging, from design to merge and release | 41 | 0 |
| `lead-replacement-board-design.json` | role replacement: the board-design Lead's seat passes twice | 40 | 2 |
| `close-with-late-fact.json` | close with late facts: #139 merged and v0.1.81 tagged, then the release result and production observations arrive | 36 | 3 |

The files are public-safe. They contain no local paths, no session or thread UUIDs, no emails and no secrets. Actors and threads are aliases; commit SHAs, PR numbers and GitHub Actions run IDs are kept because the repository is public.

## Format

```text
case, summary, sources[]          what the case is and where it came from
actors[]   {alias, kind: session|human, note}
setup[]    fleet records to create first (workspace_create, board_create)
events[]   {seq, at, actor, actingFor?, op, args, note, source?, approxTime?}
questions[] {ask, via, asOf?, expect}
probes[]   {after, actor, op, args, expect, note}   synthetic negative checks
```

### Events

Task status calls use the current `TaskStatusChange` shape (`{"kind":"done"}` for
these histories). Their completion meaning, source times, notes and expected answers
are preserved; the public API accepts no retired string-status form.

- `op` is the MCP tool name in the current catalog (`collaboration-mcp` `catalog_tools.rs`): `project_create`, `role_grant`, `workstream_create`, `workstream_close`, `task_create`, `task_set_status`, `task_link`, `task_move_workstream`, `work_trail_add`, `owner_grant_record`, `role_replace_lead`, `fact_record`, `project_close`, `thread_create` and `thread_resolve`.
- `args` uses the request's real field names and serde shapes (camelCase, tagged enums), from `message-board`'s `project_requests.rs`, `work_records.rs`, `authority_roles.rs`, `owner_grants.rs`, `fact_records.rs` and `record_references.rs`.
  - For tools that take a `ProjectRequest`, `args` is the `BoardOperation` variant's fields without `kind`; the tool name selects the variant.
  - For `project_create`, `thread_create` and `thread_resolve`, `args` is that request's own fields minus the envelope.
- The harness fills the envelope on every request: `requestId` (fresh), `projectId` (`$project`), `protocolVersion`, `actor` (the event's actor), `actingFor` (the `actingFor` alias, as `{kind: human, humanId}`, else null), `observedEpoch: null` and `resubmits: null`.
- Replay in `seq` order. Every event expects an accepted decision.
- `at` is the time from the source and is ascending. Router stamps its own time on each record (RC-9), so `at` is for ordering and reading. A fact's own time is already in `args.fact.producerTime`.
- `approxTime: true` marks a time placed by inference: a commission or a reviewer verdict that reached the Lead off-board, or a step the source implies without a timestamp. Each one's note says what it was placed relative to.
- `actingFor: "owner"` marks owner words relayed by an agent (G3, U51). The owner never writes directly.

### Placeholders

| Placeholder | Meaning |
|---|---|
| `$project`, `$board`, `$workspace` | the case's project and its setup records; the harness mints UUIDv7s |
| `$ws.*`, `$task.*`, `$row.*`, `$fact.*`, `$grant.*`, `$post.*` | an ID the harness mints the first time an event defines it (`workstreamId`, `taskId`, `rowId`, `factId`, `grantId`, `postId`); later uses resolve to the same ID. A thread's ID is its root post's ID |
| `@alias` | an actor. In an `Identity` position (`holder`, an addressee's `recipient`) it's the alias's identity. In a `RecordedActor` position (`newHolder`, `recordedBy`, `evaluator.actor`) it's `{actor: {kind: declared, identity}, installation, machine}` with the harness's installation and machine |
| `"$current"` | an expected revision: the subject's current revision (or assignment revision), read just before the call. The history never raced, so no event tests staleness. The first lead's assignment revision is 0 |
| `"$cursorOf:N"` | the epoch-qualified feed cursor of the record event N created. For a `project_close` it's the close position (`ProjectState.closedAt`). Used as late facts' causal anchors |

Record references inside links are `{kind: record, record: {kind, id}}` with a placeholder `id`; fact links are bare `{kind, id}`.

### Questions

All refs are event `seq` numbers.

- **`what was decided`** reads the decision register: `work_trail_show` with the `decisions` selection. `expect` lists every decision row, newest first. Match by ref and order; `text` is a summary, and the row's full text is in the event.
- **`where did it stop`** combines several views:
  - `projectClosed` and `closedBy`: project state;
  - `lead` and `leadHistory`: `role_list`;
  - `workstreams`, `readyQueue` and `blocked`: `view_work`. Tasks are listed per lane; the view orders them by `blockedBy`, then creation;
  - `pending`, `pendingHandbacks`, `settledHandbacks` and `settledOwnerQuestions`: PD1 §3b's pending rules. A handback is settled by a later decision row from the lead at that row's position that links to it. An owner question is settled by any later decision row linking to it. `pending` also lists open tasks;
  - `ownerWaiting`: `view_owner`;
  - `owed`: `view_lead_load` (`assess`, `closeOut`, `answerRejection`). In the replacement case it's keyed by lead;
  - `card`: `view_card` for that actor with subject `project`;
  - `lateFacts`: the facts whose `late` flag is set (`fact_list`).
- **`what proves it`** lists evidence by ref. `kind` is `test`, `ci`, `review` or `commit`, extended with `release` and `production` for release checks and production observations.
- **`asOf`** (replacement case only) means: replay events 1..asOf, ask, then continue.

### Probes

Probes aren't history. Each is a negative check that the history makes meaningful, such as a stood-down lead writing, a reused owner grant, or a write or unanchored fact after a close. Run each one on a fresh replay of events 1..`after`, so its request can't shift later feed positions. `expect.reason` is given only where spec 1 §5 pins it.

## The cases

### messaging-first-112

- **Sources:**
  - PR #112: body, 100 commits, merge `dea647b5`, one PR comment, no GitHub reviews;
  - PRs #108 and #106;
  - Actions run 37003475071 and release v0.1.59;
  - the specs in `docs/specs/2026-09-28-thread-subscriptions/` and `docs/specs/2026-09-30-router-push-format/`, and docs commit `0a996e8b`;
  - the Lead's thread-subscriptions memory note;
  - five board threads: the Lead's root, the push-format lane, the messaging milestone, the subscriptions lane and PR1 storage.
- **Why:** the case spec 1 V6 names.
- **Shape:**
  - four workstreams, with PR1 and PR2 as parallel Sidekick lanes and the #112 PR split across two Sidekicks;
  - an Opus reviewer as verifier;
  - eight decisions, three of them relayed owner words;
  - four handbacks, all settled;
  - CI, merge and release facts.
- **Where it stops:** the project is open with one task, the post-release live S3/S5/S6 check that #112 never verified.
- **Gaps:**
  - The independent reviewer's verdict and identity never appear on the board. The F1-F6 handback is rebuilt from the implementers' red-first correction receipts. The N1-N3 re-review is folded into the merge decision's text.
  - Test receipts are handback text plus commit links, not `Receipt` facts. A receipt needs a contract revision hash, obligation and scenario run that this history lacks.
  - Run IDs live in notes and text, because `EvaluatorResult` has no URL evidence.
  - The board has no matching role for:
    - the third Sidekick, commissioned for PR3+PR4 and handed over 2.5 hours later at consolidation;
    - the Workers under each Sidekick;
    - the Codex design reviewer (TS1-TS11).

    Each appears only in decision text or notes. A workstream has one implementer at a time, and Workers aren't board roles.
  - No Lead post records the merge decision. It's modelled from the merge, the PR body and the closing checkpoint.
  - The push-format workstream keeps its first name; `workstream_rename` was left out for size.
  - No moves: the project has no pinned process definition, so it has no status.

### lead-replacement-board-design

- **Sources:**
  - the board-design coordination thread and its participant history, which holds the seat replacements and their notes;
  - PR #136;
  - git history of `feat/replication-protocol` and `feat/board-model`;
  - the M2 and M3 Sidekick reports;
  - the owner rows U40-U59 in `requirements.md`.
- **Why:** the role-replacement case V6 asks for.
- **Shape:**
  - lead-a leads, settles the M1 handback, and asks and answers the lease question;
  - a protocol-types handback arrives just before the first replacement;
  - lead-b and then lead-c each take the seat through an owner grant they relay, then `role_replace_lead`;
  - lead-c settles both protocol handbacks, then the M3.6 and M2.4 handbacks;
  - owner questions are raised and answered by each lead.
- **Checks:**
  - At `asOf` (right after the second replacement), two handbacks made to earlier leads are pending and owed by lead-c. The M1 handback lead-a settled stays settled.
  - At the end nothing is owed, and one owner question is open: the Cloudflare staging account and deploy token.
- **Gaps:**
  - The source records each takeover only as an orchestrator-seat replacement with a note ("same Lead, continued in a new session after compaction" or "in a branched session"). No record carries the owner's words. Each grant is modelled as relayed by the incoming session (U51), and the reason paraphrases the seat note.
  - The protocol-types commission isn't on the board. Its time is placed before the branch's first commit.
  - The M1 and M2.4 handbacks are rebuilt from the Lead's summaries of the Sidekicks' reports.
  - The M2 and M3 slices are compressed to the M2.4 and M3.6 handbacks; slice tasks aren't modelled.
  - Advisors and reviewers outside a workstream pair appear only in text.
  - The old sessions never acknowledged their stand-down; a probe checks that refusal.

### close-with-late-fact

- **Sources:**
  - PRs #139 and #136;
  - Actions run 37956235588 (attempt 1 failed, attempt 2 succeeded) and release run 37962862929;
  - the board-design coordination thread's posts from 2026-10-08 and 2026-10-09.
- **Why this case:**
  - The closure is real: the PR merged and the tag was pushed.
  - The facts that follow matter to what closed: the release's result, then a production observation that the auto-upgraded 0.1.81 CLI couldn't reach the production Host still on 0.1.63, then the verification after the owner's restart.
  - Each fact's producer had seen the close, so each is anchored at the close position and comes out `late`.
- **Rejected candidate:** M3.6 and G18. The G18 panic observation came inside M3.6, before its acceptance; its later re-run in M3.7 followed only a slice acceptance, with no project close, so RC-11's late rule wouldn't apply.
- **Shape:**
  - before the close: three handbacks and two verifier re-verifies, all settled;
  - one owner question, answered with merge authority;
  - a CI failure that was rerun, not waived;
  - after the close: four late facts, one of them an `unmet` production observation later restored, plus an operational thread that is opened and then resolved.
- **Gaps:**
  - After a close, a work-trail row is refused (RC-11), so the owner ask to restart the Host could only be a thread post. The board has no way to raise an owner question on a closed project. A probe checks the refusal.
  - Event 33's conversation participation is inferred: the session lead declares the conversation `orchestrator` seat and watches its own operational thread, so it can later resolve it. The source does not record that conversation role. The initial `role: null` replay was refused by the kept session-participation gate; no contract was changed.
  - Release checks and production observations are recorded as `EvaluatorResult` facts produced by the Lead; no closer fact kind exists. The chief of staff's relay of the owner's restart appears only in text.
  - #137, merged under the same owner authority, isn't modelled.
  - The project's start time is placed after #136 merged.

## Assumptions

- `$current` revisions are read just before each call, and the first lead's assignment revision is 0.
- An owner grant is recorded by the incoming lead session as the relayer.
- Thread posts, replies and resolution are accepted after a project closes (settled spec 1 RC-11).
- Expectations about the stood-down leads' lead load (empty) and the card's lists assume `view_lead_load` covers only projects the actor currently leads, and that a `project` subject covers its workstreams' rows. That matches `card_projection.rs` today.
- Every expected answer was checked by replaying the events through a small model of the PD1 §3b pending rules: register order, settlement by the lead at the decision's position, owner-question settlement, ready queue, `closeOut`, late anchors and the pair-role check on handbacks.
