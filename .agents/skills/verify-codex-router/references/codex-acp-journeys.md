# Codex CLI over ACP: skills, commands and preservation (J1–J3)

**Status: draft.** The apparatus is built and passes its controlled tests, but no journey has driven a real Host, provider or installed Codex yet. Every scene's expected text comes from Codex source and is unobserved until the first declared run.

Use this journey when the change touches the app-server face's skills, slash commands or ACP provider bridge. Adopted outcomes: `docs/specs/2026-10-28-codex-acp-providers/`, Specification R1–R5 (C1–C3) and Verification Design U1–U4 (J1 skills, J2 commands, J3 preservation). A Claude pass is not a Cursor pass; qualify each provider separately.

## Entry and target

The user path is unchanged installed Codex 0.165:

```text
codex [--no-alt-screen] --remote unix://<service>/router-sessions/<endpoint>.sock \
  --cd <trusted cwd> -c features.hooks=false resume <native thread alias>
```

It runs on the shared Rust `TerminalDriver` PTY, behind the existing public wrapper, so HOME and CODEX_HOME stay the user's own. The screen is read through the wterm/libghostty grid. Original PNGs are rendered from the same PTY bytes.

Declare these before any input:

- **Candidate:** clean committed source, the isolated signed debug Host and Router from [ACP metadata](acp-metadata.md), and a private unused root, socket and port. Normal Codex home, and an existing trusted entry for `--cd`.
- **Providers:** Claude ACP 0.81.0 / SDK 0.3.280 with qualified native Claude, and Cursor 2026.10.22-27dcf5f. Confirm each Session's model before input.
- **Identities:** the raw provider SessionRef session ID (opaque) and its native thread alias. The alias is the hyphenated UUID, or `UUIDv5(URL, "codex-router:session:<service>:<endpoint>:<raw>")` (`nativeThreadAlias`). The run plan's target and the observer plan must carry the same pair. Both refuse a wrong alias before launch or readback.
- **Fixture skills:** project skills in a private provider workspace, never the owner's setup: `.claude/skills/<name>/SKILL.md` and `.cursor/skills/<name>/SKILL.md`. Each body replies `<own prefix>|<argument>|<image colour>`. That reply is absent from the typed text, the skill name and the description. Also declare the provider commands that must not appear as skills: Claude `compact`, `model`; Cursor `copy-request-id`.
- **Tools:** Node v26.12.1 (exact path and hash), frozen `scripts/verification/tui` dependencies (wterm 0.5.7), the signed `quota_storyboard_capture_test` binary, and a Chrome binding.

## Drive

1. Write the run plan (`codexAcpRunPlanSchema` in `scripts/verification/tui/codex-acp-run.ts`) and the observer plan (`journeyObservationPlanSchema` in `verification/observations/acp-journey-models.ts`).
2. Qualify without launching Codex:
   `node scripts/verification/tui/capture-run.ts --codex-acp-only --codex-acp-plan PLAN.json --check`.
   This checks the source head, artifacts, the driver signature and test entry, the emulator (including DSR, DA1 and OSC replies), and the target. The target check covers the binary hash and `--version`, Host service/epoch, endpoint socket, trusted cwd, raw→alias and observer correlation.
3. Run without `--check`. The scenes (`codex-acp-scenes.ts`) wait on grid frames:
   - **J1:** for each selected skill, `skill-picker-open-N` (the skill row and `enter insert · esc close`), then Tab to `skill-selected-N` (`$skill`). Then `skill-input-composed`: the exact composed text, and `[Image #1]` after a bracketed paste of the image path. Then Enter to `skill-effect`: every expected effect, and no `■ `. Claude takes exactly one selection; Cursor takes one or more distinct ones.
   - **J2:** `/compact`. Claude must reach `Context compacted`. Cursor must show a `■ ` error and no compaction.
   - **J3:** a long prompt reaches `Working`; Escape reaches `■ Conversation interrupted`.
   - Every journey ends with `/quit`. The Codex child must stay one process, and checkpoints must arrive in declared order.
4. Read back the same Session:
   `node verification/observations/acp-journey-observation.ts OBSERVER_PLAN.json`.
   - `idle-bind`, only after the TUI step is idle, checks `skills-listed`, `skill-effect`, `compacted` / `compact-refused` and `effective-model`.
   - `active-read-only`, during a running turn, checks only `turn-in-progress`. It never resumes or lists skills.

## Evidence and failures

Keep the run output and the observer evidence in the task's evidence home.

- **Run output:** `declared-plan.json`, `codex-target.json`, a `.grid.json` and an original PNG per checkpoint, `terminal.ansi`, `driver-commands.json`, `codex-acp-run.json` and `codex-acp-receipt.json`.
- **Observer evidence:** raw `app-server-*.json`, `provider-observation.json` and `result.json`.

Each of these is a failure:

- an empty picker, or a command listed as a skill;
- a reply that echoes or explains instead of the skill's effect;
- a Cursor first-only drop, or the wrong image colour;
- a prepared `/invocation` stored as the user's history item;
- compaction counted from an acknowledgement or title;
- a Cursor `/compact` that sends a prompt;
- a replaced Codex child, or a missing or reordered checkpoint;
- resuming a wrong alias.

## Gotchas and open items

- **Bounds:** UI checkpoints use 8 s. Model-effect checkpoints use the declared bound (initially 120 s) and span several reads, each still bounded by 8 s.
- **DA2:** Codex sends DA2 only over SSH. wterm leaves DA2 unanswered, which costs one 250 ms probe timeout.
- **Picker variant:** when mentions-v2 is enabled, its popup may replace the legacy skill popup and its hint. This is unobserved.
- **Not yet scripted:** the J2 `/model` selection keys and effective-settings path; J3 reopen, approval and active-turn refresh beyond interrupt and `turn-in-progress`.
- **Cleanup:** Codex exits through `/quit`, and the Rust entry requires child exit 0 and no survivors. Close the synthetic Sessions and stop the owned Host as [ACP metadata](acp-metadata.md) describes.
