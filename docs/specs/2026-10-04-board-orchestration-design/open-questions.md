# Owner questions: parked until after the folder moves

> Historical context. The current requirements, specifications and program designs in this folder govern. This earlier document may describe superseded choices and does not authorize implementation.

*Board/Router records Lead, 2026-10-04. The owner said these matter and asked to discuss them after the moves. Labels are stable; the first presentation was the 2026-10-04 SITREP. Updated the same day for the de-duplication with the process-definition lane and for the owner's answers relayed by the orchestration Lead.*

## Changes since the SITREP

- **Q6 (which session to replay): settled by the owner.**
  - "There's probably multiple we can extract as sessions" means several replay cases, not one pick.
  - The full replay-case extraction is the board agent's job, i.e. mine ("if not it's the board-codex-agent job"; a question-tool answer in the orchestration Lead's session, 2026-10-04 16:36Z, relayed by the process-definition lane and the orchestration Lead).
- **Q5a (milestone `dropped`) moved to the process-definition lane,** which owns its meaning. I carry only the record field.
- **Q7 (cross-machine board access) is mostly overtaken by the owner's 10-04 liaison direction:** a liaison agent per machine, and SSH to the other machine running its CLI. What's left is identity guidance for liaisons, not an owner question (see the end).
- **New to my list: Q9 (P1(b), how the owner's words are recorded)**, handed over by the process-definition lane under the de-duplication. A2 (Host restart) is also mine to raise.

## Open questions, in order of leverage

| # | Question | My recommendation | What waits |
|---|---|---|---|
| Q1 | Should Router keep a record of what hosted sessions did (commands, files, exit codes)? | (b) one row per tool call, no output, 60-day retention, capped and redacted | any check that a verifier really ran, for hosted helpers |
| Q9 | How do the owner's words (and the Lead's words to the owner) become records? | A, relayed records, now; C, a capture hook, next, sharing Q1's store and redaction rules | scored-pilot validity, Inspector authority checks, and the process-definition lane's merge-check mode |
| Q2 | Who owns the shape of the new board records? | A: Router, in Rust, with generated JSON schemas | the formal design cycle and every board build PR |
| Q3 | How should Router handle Host/client version skew? | (c) tolerate added fields; breaking changes bump the version and fail clearly with the parse detail | a small Router PR |
| Q4 | Do the renames (workspace, project) reach the API and CLI, and what is the product called? | one hard cutover with the Router build; name `agent-collaboration` | the first build PR |
| Q5 | b: is a subtask its own record? c: can small projects skip milestones? | b: yes, one level; c: PR-producing projects need ≥1 milestone, research-only ones none | the design cycle |
| Q8 | Publish cleaned design docs on the public shared branch? | design docs only, no working notes | agents on the other development machine reading the designs |

**Owner-only actions:**
- A1 (optional): reconnect the agent-router MCP in the earlier board Lead's session.
- A2 (optional): the production Host is 0.1.62 and main is 0.1.63. Upgrading needs the exact phrase "replace the production Codex router process".

### Q1: the activity record
- **What it is.** Router sees each hosted helper's commands and edits live, then discards them.
  - Codex helpers' facts survive only in Codex's store.
  - Claude and Cursor helpers' facts survive nowhere.
- **Options:**
  - (a) nothing;
  - (b) a minimal row per tool call, about 38 MB/week measured;
  - (c) full output and diffs, which is large and a privacy risk;
  - (d) read Codex's store on demand, Codex only.
- **Brief:** [2026-10-02-owner-brief-router-activity-record.md](activity-record-brief.md).

### Q9: owner words (P1(b))
- **Why it matters.** 56% of the failure signals only a transcript shows are the owner's own words: instructions, grants, corrections. Another 27% are the Lead's words to the owner. Today neither becomes a typed record, so the Inspector can't check authority, and a Lead can't legally record an owner answer that exits `waiting_on_owner`.
- **Options:**
  - **A. A relayed record.** The Lead posts the owner's words verbatim, `actingFor` the owner. It is shown as relayed and never satisfies a merge grant.
  - **B. The owner posts on the board.** This isn't first-hand today, because human-actor posts are unauthenticated.
  - **C. A capture hook in the Lead's terminal** records the owner's turns automatically. It shares Q1's store and redaction rules.
- **Recommendation:** A now, C next, decided together with Q1.
- **Detail:** the spec, §9 "Open (owner decision)".

### Q2: the contract home (queued as 9.1)
- A, Router owns the shapes: one validator, typed rejections at write; every shape change is a Router release.
- B, skills own JSON schemas: faster, but Router can't reject bad records at write.
- C, each repo defines its own: the shapes drift.

### Q3: version skew
- **Evidence:** two outages this week. A required manifest field was added without a version bump. A new reply field (`postedAsRole`) broke MCP processes started before the upgrade.
- **Runbook:** debugging/2026-10-02-cli-manifest-read-invaliddata.md (unpublished source).
- **On the hard-cutover rule.** (c) keeps no second code path; it only stops added fields from breaking older readers.

### Q4: renames and the product name
- **Option (a), rename with the build:** every skill and agent breaks once.
- **Option (b), docs only:** agents see "topic" where the docs say "project" forever.
- **The name:** "codex-router" undersells it; the CLI is already `agent-collaboration`.

### Q8: publication
- **What it is:** one commit of cleaned copies (no local paths, session ids or private links) of the spec, entity model, activity-record brief and learnings report on `codex/shared-router-design-2026-10-03`.
- **Excluded:** working notes, the handover, debugging runbooks and private working files.

## Guidance, not a question: identity over SSH (ex-Q7)

An agent that SSHes into another machine and runs that machine's `agent-collaboration` has no implicit session identity there (`--actor self` reads the local session env). It must pass its own SessionRef (`--from`), not borrow a local session's. The receiving Router can't wake a foreign session, so replies have to be checked for. I'll give this to the liaisons when cross-machine posting starts.
