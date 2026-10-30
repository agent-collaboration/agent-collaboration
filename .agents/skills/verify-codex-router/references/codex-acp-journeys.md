# Codex CLI over ACP: skills, commands and preservation (J1–J3)

**Status: draft.** The Cursor J1 `skill` TUI path (picker rows, selection, composed input and the provider-owned effect) has been driven once on a real Host. Its normal exit and the supporting readback are not yet qualified. The `picker` journey, Claude J1, and all of J2 and J3 are unrun. Scene text not yet seen on a real screen comes from Codex source and stays unobserved until a declared run. Run records belong in the task evidence, not here.

Use this journey when the change touches the app-server face's skills, slash commands or ACP provider bridge. Adopted outcomes: `docs/specs/2026-10-28-codex-acp-providers/`, Specification R1–R5 (C1–C3) and Verification Design U1–U4 (J1 skills, J2 commands, J3 preservation). A Claude pass is not a Cursor pass; qualify each provider separately.

## Entry and target

The user path is unchanged installed Codex 0.165:

```text
codex [--no-alt-screen] --remote unix://<service>/router-sessions/<endpoint>.sock \
  --cd <trusted cwd> -c features.hooks=false \
  -c 'tui.status_line=["model-with-reasoning"]' resume <native thread alias>
```

It runs on the shared Rust `TerminalDriver` PTY, behind the existing public wrapper, so HOME and CODEX_HOME stay the user's own. The `tui.status_line` override applies to this launch only and leaves global config untouched. A user status line may include `context-window-size`, which Codex fills from local config when the provider reports no window, and Cursor reports none. The footer then shows the model alone. This is a presentation choice; it does not stop Codex using that fallback. The screen is read through the wterm/libghostty grid. Original PNGs are rendered from the same PTY bytes.

Declare these before any input:

- **Candidate:** clean committed source, the isolated signed debug Host and Router from [ACP metadata](acp-metadata.md), and a private unused root, socket and port. Normal Codex home, and an existing trusted entry for `--cd`.
- **Providers:** Claude ACP 0.81.0 / SDK 0.3.280 with qualified native Claude, and Cursor 2026.10.22-27dcf5f. Confirm each Session's model before input.
- **Identities:** the raw provider SessionRef session ID (opaque) and its native thread alias. The alias is the hyphenated UUID, or `UUIDv5(URL, "codex-router:session:<service>:<endpoint>:<raw>")` (`nativeThreadAlias`). The run plan's target and the observer plan must carry the same pair. Both refuse a wrong alias before launch or readback.
- **Working directories:** two, and they may differ. The launch cwd is the trusted directory Codex gets with `--cd`. The Session cwd is the provider Session's own working directory, often its private fixture workspace. The observer plan declares the Session cwd. The observer refuses unless native `thread/read` and `thread/resume` report that same `cwd`, and it lists skills there.
- **Fixture skills:** project skills in a private provider workspace, never the owner's setup: `.claude/skills/<name>/SKILL.md` and `.cursor/skills/<name>/SKILL.md`. Each body replies `<own prefix>|<argument>|<image colour>`. That reply is absent from the typed text, the skill name and the description. Also declare the provider commands that must not appear as skills: Claude `compact`, `model`; Cursor `copy-request-id`.
- **Tools:** Node v26.12.1 (exact path and hash), frozen `scripts/verification/tui` dependencies (wterm 0.5.7), the signed `quota_storyboard_capture_test` binary, and a Chrome binding.

## Drive

1. Write the run plan (`codexAcpRunPlanSchema` in `scripts/verification/tui/codex-acp-run.ts`) and the observer plan (`journeyObservationPlanSchema` in `verification/observations/acp-journey-models.ts`).
2. Qualify without launching Codex:
   `node scripts/verification/tui/capture-run.ts --codex-acp-only --codex-acp-plan PLAN.json --check`.
   This checks the source head, artifacts, the driver signature and test entry, the emulator (including DSR, DA1 and OSC replies), and the target. The target check covers the binary hash and `--version`, Host service/epoch, endpoint socket, trusted cwd, raw→alias and observer correlation.
3. Run without `--check`. The plan declares one journey `kind`. The scenes (`codex-acp-scenes.ts`) wait on grid frames. Typed text arrives as one PTY write, which Codex buffers as a paste burst; an Enter sent before the next frame becomes a newline. So no Enter follows typed text until a frame shows that text taking effect: a command's slash-popup row, or the prompt in the composer.
   - **J1 `skill`:** for each selected skill, `skill-picker-open-N` (the selected `› <name>  [Skill]` row and `enter insert · esc close`), then Tab to `skill-selected-N` (`$skill`). Then `skill-input-composed`: the exact composed text, and `[Image #1]` after a bracketed paste of the image path. Then Enter to `skill-effect`: every expected effect, and no `■ `. Claude takes exactly one selection; Cursor takes one or more distinct ones.
   - **J1 `picker`:** R1 with no model input. `skill-picker-open-1` (the selected skill row), then Ctrl+C to `draft-cleared` (`… again to quit`, popup gone). Ctrl+C on a non-empty composer clears it and arms the quit hint. Nothing is submitted, so the provider sees no prompt and no credential is used.
   - **J2 `compact`:** `/compact` to `compact-command-ready` (its row, `summarize conversation to prevent hitting the context limit`), then Enter. Claude must reach `Context compacted`. Cursor must show a `■ ` error and no compaction.
   - **J2 `model`:** `/model` to `model-command-ready`, Enter to `model-popup-open` (`Select Model` or `Select Model and Effort`), then the declared navigation keys and the apply key to `model-applied`: the declared outcome text, with no model or `Select Reasoning Level` popup still open. `apply: session` presses plain `s` on the row. That sends one `thread/settings/update` with the model and effort, writes no config, and shows `Model changed to <model>[ <effort>][ <tier>] for this session only`, or `Failed to update thread settings: …` when refused. `apply: default` presses Enter, which also saves a Codex default. An ACP endpoint refuses that save: `Failed to save default model: … Saving Codex configuration/defaults is not supported on this ACP endpoint`. Declare that refusal as the outcome; never fake persistence. The model, its row and its keys come from the Session's actual advertised list. The outcome text never proves the effective model; the observer's `effective-model` does.
   - **J3 `interrupt`:** the long prompt to `prompt-composed` (in the composer, not yet `Working`), Enter to `turn-running` (`Working`), then Escape to `turn-interrupted` (`■ Conversation interrupted`).
   - **J3 `reopen`:** no input. The resume screen reaches `history-replayed`: every declared line of the reply this Session already holds. Only the visible screen is read, not scrollback, so declare the latest reply of a short Session. The run plan and the observer address the same raw Session and native alias.
   - Every journey ends with Codex's own exit: type `/quit`, wait for its slash-popup row (`exit Codex`), then Enter, then a bounded wait for PTY EOF. The Codex child must stay one process, and checkpoints must arrive in declared order.
4. Read back the same Session:
   `node verification/observations/acp-journey-observation.ts OBSERVER_PLAN.json`.
   Run it directly. Its own bounds cover every wait: `events observe` 20 s, WebSocket handshake 10 s, 15 s per response. Do not wrap it in `timeout`, which macOS lacks.
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
- **`/model` service tier:** with no tier from the picker, the session apply still adds `serviceTier` when the Codex profile enables Fast or another tier (`service_tier_resolution`). Inspect the actual profile and request before the run. If the endpoint refuses `serviceTier`, the honest outcome is the `Failed to update thread settings` refusal: declare it and report it unsupported. Never drop the field. A session apply from a `Select Model` row uses that model's default effort; it does not establish per-model defaults or reset semantics.
- **Not yet scripted:** J3 approval and active-turn refresh. Approval needs a provider tool call in a permission mode that asks, and model input; Router forwards it as a native server request, and Codex's approval overlay answers it. Active-turn refresh needs a mid-turn command or skill change. Cursor publishes its commands only at new or load, so it has none to drive. Claude needs a long-running turn and its credentials. Until then, the observer's `active-read-only` `turn-in-progress` is the only active-turn check.
- **Cleanup:** Codex exits through `/quit`, and the Rust entry requires child exit 0 and no survivors. Close the synthetic Sessions and stop the owned Host as [ACP metadata](acp-metadata.md) describes.
