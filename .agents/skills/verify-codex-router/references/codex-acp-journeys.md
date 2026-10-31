# Codex CLI over ACP: skills, commands and preservation (J1–J3)

**Status: partially qualified; the journey set as a whole is still a draft.** These are qualified on an isolated Host with unchanged installed Codex 0.165 at 120×36 and legacy keys, each at the scope stated:

- **J1 `skill`:** Cursor with two selected skills and Claude with one, each with an argument and an image. Covers picker rows, selection, composed input, the provider-owned effect, normal exit and the same-Session readback (`skills-listed`, `skill-effect`).
- **J1 `picker`:** Claude and Cursor, with no model input. Covers the selected row, the cleared draft and normal exit.
- **J3 `reopen`:** Claude, with no new prompt. The existing reply is replayed and Codex exits normally.
- **J2 `model` with `apply: default`:** Claude, on the earlier free-navigation journey against the bridge's hardcoded catalogue. The popup changed the model, the honest default-save refusal was shown, and `effective-model` read back the chosen model. The typed catalogue journey and `model-catalogue` are not yet qualified.
- **J2 `compact` on Cursor:** an explicit unsupported error, with no compaction effect, unchanged history, a usable empty composer and normal exit. This needs a Router build that reports unsupported compaction without closing Codex; on an older build the refusal ends the CLI.
- **Readback route:** the Claude J1 and J2 `model` readbacks, both picker readbacks and the Cursor compact readbacks ran through the observer's CLI entry (`acp-journey-observation.ts PLAN.json`).

Not yet qualified:

- J2: Claude `compact` (`Context compacted`), `model` with `apply: session`, and `model` on Cursor.
- J3: `interrupt`, Cursor `reopen`, approval, active-turn refresh, and native and reverse control preservation.
- A next valid prompt after a Cursor compact refusal; only a controlled Host test covers it.
- Model-specific effort defaults. Per-model default provenance for a model that isn't current isn't observable through public APIs: `session inspect` shows only the current model's effort option, and a live current value may be a saved setting rather than the vendor default. It stays a source and decoder integration obligation. The UI's `(default)` marker reflects only the native list.
- Provider context capacity. The footer shows the model only and says nothing about capacity.
- Cursor J1 readback through the observer's CLI entry. That readback called the exported `runJourneyObservation` with a schema-validated plan instead.

Nothing here is a release or PR readiness claim. Scene text not yet seen on a real screen comes from Codex source and stays unobserved until a declared run. Run records belong in the task evidence, not here.

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
   - **J1 `picker`:** R1 with no model input. `skill-picker-open-1` (the selected skill row), then Ctrl+C to `draft-cleared`. That is a grid line holding only the empty composer, `› Ask Codex to do anything`, with the popup hint gone. Codex 0.165 shows no "again to quit" hint, because that reminder is compiled off. Ctrl+C clears a non-empty draft, but on an empty composer it quits Codex at once, so it is sent only after the picker frame proves the draft is drawn. Nothing is submitted, so the provider sees no prompt and no credential is used.
   - **J2 `compact`:** `/compact` to `compact-command-ready` (its row, `summarize conversation to prevent hitting the context limit`), then Enter. Claude must reach `Context compacted`. Cursor must show a `■ ` error and no compaction.
   - **J2 `model`:** `/model` to `model-command-ready`, then Enter. Codex draws its cached bootstrap catalogue first and replaces it when its model refresh returns, so no key is sent until `model-catalogue-ready`: `Select Model`, every declared row of the visible page (at most 8) and none of the declared stale bootstrap rows. Each declared batch of Up/Down keys is sent in order, and its final selected row must then be drawn before the next step's key: `› <number>. <display name>` with exactly the declared `(default)` and `(current)` markers, ending at the line end or at the two-cell gap before the description column. Rows passed on the way are not observed. The target model is reached with Up/Down only (`model-row-selected`). A model with several plain efforts then opens `Select Reasoning Level for <display name>` (`reasoning-popup-open`, then `effort-selected`); one plain effort or none applies directly. Effort rows carry Codex's label for the native value (`xhigh` is `Extra high`, an unknown value is shown raw), never the provider's label, and Max/Ultra sit behind `More reasoning…` and are out of scope. The apply key then reaches `model-applied`: the declared outcome with no model or reasoning popup open. `apply: session` presses plain `s`: one `thread/settings/update`, no config write, `Model changed to <model>[ <effort>][ <tier>] for this session only`, or `Failed to update thread settings: …` when refused. `apply: default` presses Enter, which also saves a Codex default. An ACP endpoint refuses that save with `Failed to save default model: …` and its refusal; declare that refusal as the outcome and never fake persistence. Declare short outcome fragments that each fit one row. The catalogue page, stale rows, row numbers and effort rows come from the observer's actual unbound and bound `model/list` for the same Session; the outcome text never proves the effective model.
   - **J2 `rename` (diagnostic, not a delivery gate):** `/rename <fixture title>` to `rename-command-ready`: the composed line, with `Resuming session…` gone (the startup draft accepts typing before the resume lands). The slash popup closes once a title follows the command. Then Enter to `rename-refused`: the endpoint's declared refusal as an error cell, an empty composer, then a normal `/quit`. The adopted outcomes require a usable composer after an unsupported command only for Cursor `/compact`, so this probe only reports whether Codex stays usable. The current client exits when `thread/name/set` is refused; that is a known unsupported-command client limitation, not a recovery and not a missing rename accepted as done. The probe never stands in for a rename capability. The title is harmless text: letters, digits, spaces, `_` or `-`, at most 40 characters, no padding.
   - **J3 `interrupt`:** the long prompt to `prompt-composed` (in the composer, not yet `Working`), Enter to `turn-running` (`Working`), then Escape to `turn-interrupted` (`■ Conversation interrupted`).
   - **J3 `reopen`:** no input. The resume screen reaches `history-replayed`: every declared line of the reply this Session already holds. Only the visible screen is read, not scrollback, so declare the latest reply of a short Session. The run plan and the observer address the same raw Session and native alias.
   - Every journey ends with Codex's own exit: type `/quit`, wait for its slash-popup row (`exit Codex`), then Enter, then a bounded wait for PTY EOF. The Codex child must stay one process, and checkpoints must arrive in declared order.
4. Read back the same Session:
   `node verification/observations/acp-journey-observation.ts OBSERVER_PLAN.json`.
   Run it directly. Its own bounds cover every wait: `events observe` 20 s, WebSocket handshake 10 s, 15 s per response. Do not wrap it in `timeout`, which macOS lacks.
   - `idle-bind`, only after the TUI step is idle, checks `skills-listed`, `skill-effect`, `compacted` / `compact-refused`, `effective-model` and `model-catalogue`. With `model-catalogue` it also reads native `model/list` before and after its own resume and the provider Session's own catalogue through public `session inspect`, keeping every raw response. The check requires every provider model in the bound list (only the declared delegation may be extra), a bootstrap list of the delegation alone, and the provider's current model as the native one. For `effort.kind: qualified` the selected model's native choices must equal the provider's effort options, its default must be an offered value, and the current values must agree. For `provider-settings` the row must carry exactly the declared action, which is never accepted as the effective effort. The native list is never its own reference.
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
- **Row width:** at 120 columns a Claude skill row's trailing source suffix clips at the right edge. The selected-row oracle reads only the prompt, the display name and the `[Skill]` tag, so the clip doesn't change the verdict.
- **`/model` service tier:** the picker's session apply carries no tier. `select_session_model` then sends whatever `service_tier_update_for_core` returns (`service_tier_resolution.rs:75-97`):
  - the effective tier, if any: a configured `service_tier`, `default` after a Fast opt-out notice, or the model's supported default tier, filtered by the enabled tier features;
  - otherwise, when the Fast or Ultrafast feature is enabled and the model is in the catalog, `serviceTier: "default"`, even with no configured tier;
  - otherwise no `serviceTier` field at all.

  Before the run, inspect the actual profile, its features and the request. If the endpoint refuses `serviceTier`, the honest outcome is the `Failed to update thread settings` refusal: declare it and report it unsupported. Never drop the field or map a tier. A session apply from a `Select Model` row uses that model's default effort; it does not establish per-model defaults or reset semantics.
- **Not yet scripted:** J3 approval and active-turn refresh. Approval needs a provider tool call in a permission mode that asks, and model input; Router forwards it as a native server request, and Codex's approval overlay answers it. Active-turn refresh needs a mid-turn command or skill change. Cursor publishes its commands only at new or load, so it has none to drive. Claude needs a long-running turn and its credentials. Until then, the observer's `active-read-only` `turn-in-progress` is the only active-turn check.
- **Cleanup:** Codex exits through `/quit`, and the Rust entry requires child exit 0 and no survivors. Close the synthetic Sessions and stop the owned Host as [ACP metadata](acp-metadata.md) describes.
