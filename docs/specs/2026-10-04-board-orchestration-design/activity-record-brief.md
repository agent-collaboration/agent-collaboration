# Owner brief: should Router keep a record of what hosted sessions did?

> Historical context. The current requirements, specifications and program designs in this folder govern. This earlier document may describe superseded choices and does not authorize implementation.

*From the board Lead, 2026-10-02. One decision. It's a storage boundary, so it's yours.*

## 1. The decision

**Should Agent Router keep its own durable record of what each session it hosts actually did — the files it edited, the commands it ran, and the exit codes — so the orchestrator can check work after the fact?**

## 2. Today

```text
            ┌──────────────── a session's actions ─────────────────┐
            │  edits · commands · verifier runs (+ exit codes)     │
            └───┬──────────────────┬──────────────────────┬────────┘
                │                  │                      │
   Claude Code terminal   Router-hosted Claude     Router-hosted Codex
   (not hosted)           or Cursor helper         helper (codex-local)
                │                  │                      │
          hooks only:      Router sees it LIVE:    Router sees it LIVE:
          edit + bash      kind + status + title   full item (command,
          facts, if the    (content dropped),      output, diff, exit)
          hooks run        held IN MEMORY          — not stored
                │                  │                      │
                ▼                  ▼                      ▼
          nothing kept      lost on restart,       Codex's own store
          by Router         close or resume        keeps it; readable
                                                   via Router's bridge
                └──────────────┬───────────┴──────────────┘
                               ▼
              Router keeps: board posts · lifecycle journal
              (turn ended / status, no content, 30 days) ·
              delivery receipts. No edits, commands or exits.
```

What this means today:

- After the fact, Router **cannot show** what any session did. Its lifecycle journal records only lifecycle facts (`lifecycle_observation.rs:1`). Its event hub keeps events in memory only (`provider_session_event_hub.rs:1`) and drops tool content (`provider_item_projection.rs:168`).
- **Measured, unhosted Leads** (6 audited sessions): Router saw **24%** of the Leads' actions, and **none** of their 2,009 file edits.
- **Measured, hosted Codex helpers** (132 threads, read through the bridge):
  - 94,875 commands ran.
  - 15,614 files changed.
  - There were 12,174 verifier runs, all with exit codes; **1 in 3 failed**.
  - All of this exists only in Codex's own store.
- Codex runs **no hooks** under Router, so hooks can't fill the gap for Codex helpers.

## 3. The options

### (a) Keep nothing; rely on push

```text
  hooks (Claude only) ──► board facts      receipts posted by agents ──► board
  Router copy: none
```

| | |
|---|---|
| **Gains** | No new storage, no new privacy surface. |
| **Costs** | Codex-hosted work stays invisible to the orchestrator. Proof depends on agents posting honest receipts. The tier-1 limit applies: a fabricated receipt passes. |
| **Who pays** | The Lead and you: you check by asking, or by reading logs. |

### (b) Keep a minimal activity record (my recommendation)

```text
  live item ──► Router writes ONE row per tool call:
                kind · paths · command · exit status · time · session
                (no output bodies, no diffs)   retention: e.g. 60 days
```

| | |
|---|---|
| **Gains** | One record across Codex, Claude and Cursor hosted sessions. The orchestrator and the Inspector can check "did a verifier really run, and did it pass?" without trusting a receipt. This closes the Codex no-hook gap. |
| **Costs** | New Router storage plus a retention policy. **Measured:** 79,618 command rows and 13,259 file-change rows over 8 days of helper work, about **38 MB a week** raw (1.5–2× with indexes). Command lines are 168 bytes at p50 and 591 at p95; one reached 40 KB, so cap the field. Command lines can contain sensitive arguments, so a redaction rule is needed. For Claude-hosted sessions, how much detail the adapter exposes is **unverified**. |
| **Who pays** | Router: storage, code and a retention job. |

### (b+) Minimal record plus the agents' own messages, for hosted sessions only

```text
  (b) PLUS each hosted agent's messages (assistant text, not tool output)
```

| | |
|---|---|
| **Gains** | **Little.** This captures the words of HOSTED helpers. The failures that needed words came from unhosted Lead terminals, which (b)/(b+) never see, and 0% of those words were agent-to-agent (E13 follow-up). |
| **Costs** | More storage than (b), plus a privacy cost: agent text can quote secrets. |
| **Who pays** | Router. |

*Corrected 2026-10-02 (pointed out by the process-definition lane): an earlier version credited (b+) with the 63-failure word evidence. That evidence comes from unhosted Leads, so it belongs to the next section.*

> **The words that matter come through a different path: P1(b) and record-first.**
> - The 63 audited failures come from **unhosted** Lead terminals: Claude Code sessions you talk to directly. Router alone detects 3 and hooks raise that to 29; the rest need words.
> - **Whose words** (E13 follow-up, 48 signals that only a transcript can show):
>   - 56% are **your own chat words**: instructions, corrections, grants, or their absence.
>   - 27% are the Lead's words **to you**: proposals, claims.
>   - 0% are the Lead's words to other agents.
>   - 17% could be config or code instead.
> - So the text channel the judge needs is **your conversation with the Lead**, and it needs:
>   - **P1(b)**: your words recorded as typed, retrievable owner-word records (instruction, grant, correction, agreement; verbatim; first-hand or relayed; scope; refs to the exact PR or task). They get there either by the Lead relaying them, or by a hook in the Lead's own terminal capturing your turns;
>   - **record-first** for the Lead's claims: done claims, owner briefs and moves made as typed board records.
> - **Confirmed by E11b/c:** every checker caught 38% or less, and a reasoning agent did worse than narrow questions. The missing input was your grants, instructions and agreements, which lay outside the checker's window. **The Inspector needs retrieval of your recorded words, not a smarter judge.** P1(b), not (b+), unlocks most authority checks.

### (c) Keep a full record

```text
  live item ──► Router stores (b) PLUS command output and file diffs
```

| | |
|---|---|
| **Gains** | Everything is in one place, and full replay is possible. |
| **Costs** | Large storage, and a real privacy surface (output can contain secrets). It duplicates what Codex already stores. |
| **Who pays** | Router and you: privacy and disk. |

### (d) Keep no copy; read Codex's own store when needed

```text
  orchestrator ──► Router bridge ──► Codex store (thread/turns/list, full)
  Router copy: none
```

| | |
|---|---|
| **Gains** | No new Router storage. It works today for Codex helpers, as the orchestration Lead just did. |
| **Costs** | Codex only: Claude and Cursor hosted sessions have no equivalent store Router can read. The format is provider-specific. Retention is Codex's, not ours. Reading 132 threads takes a heavy paged scan. |
| **Who pays** | Whoever runs the check each time. |

## 4. My recommendation: (b), not (b+). Decide P1(b) for the words.

- **New evidence (E11):** suggesting the next move needs only board state. Router-supported inputs got it right 83% of the time, versus 76% with full transcripts.
- **Detecting failures needs two inputs:**
  - action facts: (b) for hosted helpers, hooks for unhosted Leads;
  - your words: P1(b).
  (b+) adds little.

- It is the smallest change that makes proof checkable without trusting the agent's word, for every hosted provider rather than Codex only.
- Router already sees these items live; it just throws them away.
- Output bodies stay in each provider's own store. Option (d) remains the deep-dive path for Codex when someone needs the actual output.
- It also turns E13's "24% visible" into a measured, per-session number going forward.

## 5. If you defer

- **Blocked:** nothing in the current pilot. The pilot uses hooks and posted receipts, and the orchestration Lead can read Codex helpers through the bridge (option d) for analysis.
- **Waits:** the "session → transform → decision" design for checking the Lead's reviewing (idea 6), and any check that a verifier really ran. Both stay at tier 1 (self-reported) for hosted sessions until this is decided.

*Sources: Router code read 2026-10-02 (file:line citations in design notes §16 (unpublished source)); E13 results in `jev-experiments.md` (the orchestration Lead's experiments); a lane post on the board-design thread.*
