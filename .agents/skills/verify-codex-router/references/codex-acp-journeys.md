# Codex CLI over ACP: skills, commands and preservation (J1–J3)

**Status: partially qualified; the journey set as a whole is still a draft.** These observations used an isolated Host with unchanged installed Codex 0.165 at 120×36 and legacy keys. Historical passes do not qualify a newer candidate when their producer or runtime dependencies changed:

- **J1 `skill`:** earlier Cursor `cd9668cd5` with two selected skills and Claude `828378c81` with one, each with an argument and an image. Covers picker rows, selection, composed input, the provider-owned effect, normal exit and the same-Session readback (`skills-listed`, `skill-effect`). These are not successful skill/image replies on the later model-metadata candidate.
- **J1 `picker`:** Claude and Cursor, with no model input. Covers the selected row, the cleared draft and normal exit.
- **J3 `reopen`:** Claude, with no new prompt. The existing reply is replayed and Codex exits normally.
- **J2 `model` with `apply: default`:** the earlier Claude free-navigation journey changed the model against the hardcoded catalogue. On `6b9d2c20f`, the typed refreshed-catalogue journey and same-session `model-catalogue` readback separately passed for Sonnet/low and Haiku/low, preserving all five advertised model IDs and showing the honest default-save refusal. Each had six original terminal captures and normal quit. This qualifies selection/current-value readback only: both subsequent skill/image prompts failed to produce their required replies, later returning ACP `-32603/server_error`. The cause and the complete model-plus-prompt gate remain open.
- **J2 `compact` on Cursor:** an explicit unsupported error, with no compaction effect, unchanged history, a usable empty composer and normal exit. This needs a Router build that reports unsupported compaction without closing Codex; on an older build the refusal ends the CLI.
- **J2 `information` on Claude:** on `c00692d8e`, `/hooks` and `/mcp` each showed their command-specific refusal, returned to an empty usable composer and quit normally. Each paired same-session observer confirmed the exact native error and a subsequent supported read. This proves refusal recovery only, not hook discovery or provider MCP inventory. Cursor is unqualified.
- **Readback route:** the Claude J1 and J2 `model` readbacks, both picker readbacks and the Cursor compact readbacks ran through the observer's CLI entry (`acp-journey-observation.ts PLAN.json`).

Not yet qualified:

- J2: Claude `compact` (`Context compacted`), `model` with `apply: session`, and `model` on Cursor.
- J2 `information` (`/hooks`, `/mcp`) on Cursor remains unobserved. On `c00692d8e`, the actual Haiku/low `prompt` control submitted one plain Text message with no skill or image, but no required reply appeared within 120 seconds; ACP returned `-32603/server_error` at about 186.6 seconds. Successful basic inference after model selection is unqualified; the cause and actual downstream route remain unknown. No Sonnet control was sent.
- J3: `interrupt`, Cursor `reopen`, approval, `refresh` (scripted from source, with no real target or setup chosen yet), a Claude skill added mid-turn, and native and reverse control preservation.
- `ordinary-reply` and `information-refused`: observer checks exercised only on literal history fixtures and the controlled transport.
- `claude_route` and `claude-route`: the route window and its managed-Session isolation are exercised only on literal audit rows and inventory pages, real temporary files, a controlled collaboration CLI stand-in and the controlled shared-driver stand-in. Neither has run against a real Host or CLI, so routed Claude inference (Q2) stays unqualified. The runner's `--check` CLI and manifest binding is typechecked only.
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
3. Run without `--check`. The plan declares one journey `kind`. The scenes wait on grid frames: shared matchers and most journeys in `codex-acp-scenes.ts`, and `model`, `information` and `refresh` in `codex-acp-model-scenes.ts`, `codex-acp-information-scenes.ts` and `codex-acp-refresh-scenes.ts`. Typed text arrives as one PTY write, which Codex buffers as a paste burst; an Enter sent before the next frame becomes a newline. So no Enter follows typed text until a frame shows that text taking effect: a command's slash-popup row, or the prompt in the composer.
   - **J1 `skill`:** for each selected skill, `skill-picker-open-N` (the selected `› <name>  [Skill]` row and `enter insert · esc close`), then Tab to `skill-selected-N` (`$skill`). Then `skill-input-composed`: the exact composed text, and `[Image #1]` after a bracketed paste of the image path. Then Enter to `skill-effect`: every expected effect, and no `■ `. Claude takes exactly one selection; Cursor takes one or more distinct ones.
   - **J1 `picker`:** R1 with no model input. `skill-picker-open-1` (the selected skill row), then Ctrl+C to `draft-cleared`. That is a grid line holding only the empty composer, `› Ask Codex to do anything`, with the popup hint gone. Codex 0.165 shows no "again to quit" hint, because that reminder is compiled off. Ctrl+C clears a non-empty draft, but on an empty composer it quits Codex at once, so it is sent only after the picker frame proves the draft is drawn. Nothing is submitted, so the provider sees no prompt and no credential is used.
   - **J2 `compact`:** `/compact` to `compact-command-ready` (its row, `summarize conversation to prevent hitting the context limit`), then Enter. Claude must reach `Context compacted`. Cursor must show a `■ ` error and no compaction.
   - **J2 `model`:** `/model` to `model-command-ready`, then Enter. Codex draws its cached bootstrap catalogue first and replaces it when its model refresh returns, so no key is sent until `model-catalogue-ready`: `Select Model`, every declared row of the visible page (at most 8) and none of the declared stale bootstrap rows. Each declared batch of Up/Down keys is sent in order, and its final selected row must then be drawn before the next step's key: `› <number>. <display name>` with exactly the declared `(default)` and `(current)` markers, ending at the line end or at the two-cell gap before the description column. Rows passed on the way are not observed. The target model is reached with Up/Down only (`model-row-selected`). A model with several plain efforts then opens `Select Reasoning Level for <display name>` (`reasoning-popup-open`, then `effort-selected`); one plain effort or none applies directly. Effort rows carry Codex's label for the native value (`xhigh` is `Extra high`, an unknown value is shown raw), never the provider's label, and Max/Ultra sit behind `More reasoning…` and are out of scope. The apply key then reaches `model-applied`: the declared outcome with no model or reasoning popup open. `apply: session` presses plain `s`: one `thread/settings/update`, no config write, `Model changed to <model>[ <effort>][ <tier>] for this session only`, or `Failed to update thread settings: …` when refused. `apply: default` presses Enter, which also saves a Codex default. An ACP endpoint refuses that save with `Failed to save default model: …` and its refusal; declare that refusal as the outcome and never fake persistence. Declare short outcome fragments that each fit one row. The catalogue page, stale rows, row numbers and effort rows come from the observer's actual unbound and bound `model/list` for the same Session; the outcome text never proves the effective model.
   - **J2 `rename` (diagnostic, not a delivery gate):** `/rename <fixture title>` to `rename-command-ready`: the composed line, with `Resuming session…` gone (the startup draft accepts typing before the resume lands). The slash popup closes once a title follows the command. Then Enter to `rename-refused`: the endpoint's declared refusal as an error cell, an empty composer, then a normal `/quit`. The adopted outcomes require a usable composer after an unsupported command only for Cursor `/compact`, so this probe only reports whether Codex stays usable. The current client exits when `thread/name/set` is refused; that is a known unsupported-command client limitation, not a recovery and not a missing rename accepted as done. The probe never stands in for a rename capability. The title is harmless text: letters, digits, spaces, `_` or `-`, at most 40 characters, no padding.
   - **J3 `interrupt`:** the long prompt to `prompt-composed` (in the composer, not yet `Working`), Enter to `turn-running` (`Working`), then Escape to `turn-interrupted` (`■ Conversation interrupted`).
   - **J2 `information`:** `command` is `hooks` or `mcp` and nothing else: no arguments, `login`, options or free slash text. The default inventory flow sends `hooks/list` or `mcpServerStatus/list`. Type `/<command>` to reach `<command>-command-ready`, the selected slash row `› /<command>`, the column gap, then exactly its description. That frame forbids the refusal line, so an error left by earlier input in this TUI cannot pass, and forbids `Resuming session…`. Enter then reaches `<command>-refused`, which needs three things:
     - one whole error line at the line start: `■ Failed to load hooks: hooks/list failed in TUI` or `■ Failed to load MCP inventory: mcpServerStatus/list failed in TUI`;
     - for `/mcp`, no `Loading MCP inventory`;
     - an empty composer, which also means the slash popup closed and Enter dispatched the command: Codex draws that popup only while the first line holds a `/` command name being edited (`chat_composer.rs:3966-3990`).

     A normal `/quit` follows. Codex shows the refused request's error with its plain Display, which for an eyre report is only the outermost context (`hooks_rpc.rs:35`, `app/background_requests.rs:327-333`, `:742-747` and `:803`; eyre 0.6.12 `EyreHandler::display`). So every refusal of that request draws the same line, and the endpoint's code and message (`Hook discovery is not supported on this ACP endpoint` or `-32601`) are never on screen. The journey proves only that Codex surfaced the refusal and stayed usable. Keep `features.hooks=false` and the normal home: `/hooks` is offered regardless and only reads.

     **Two proof layers, paired.** The TUI layer above proves Codex surfaced a new command-specific refusal and stayed usable; it cannot prove the endpoint's code or message. The observer's `information-refused` check proves those. It is idle-bind only, after the TUI step, on the observer's own bound connection to the same Session. It sends the command's request exactly once, with no retry:
     - `hooks/list` with `{cwds: [<Session cwd>]}`;
     - `mcpServerStatus/list` with `{cursor: null, limit: 100, detail: "toolsAndAuthOnly", threadId: <alias>, serverName: null}`, as Codex 0.165 sends it (`app/background_requests.rs:780-803`).

     It requires the response for that request ID to be a well-formed error: `-32000` `Hook discovery is not supported on this ACP endpoint`, or `-32601` `Method not found` (`router_session_app_server_methods.rs:100-108`). A following `thread/read` on the same connection must still return this thread and cwd. The raw error is kept as that request's `app-server-<id>.json`. Declare both layers for each command. Neither alone is the outcome, and controlled transport tests are not live J2 proof.
   - **J2 `prompt`:** the next ordinary prompt, such as the one after a `/model` selection. The plan declares only `provider`, `prompt`, `completion` and `reply_deadline_ms`.
     - **Prompt and completion:** the same rules as `refresh`. Prefer an arithmetic control, so the expected integer (at least 8 digits) is never typed.
     - **Frames:**
       1. `prompt-composed`: the exact prompt drawn as `› <prompt>`, with no completion, `■ ` cell, `Working` or `Resuming session…` on screen.
       2. Enter to `prompt-replied`: the completion and an empty composer, with no `Working` and no `■ ` cell. The composed frame showed none, so any error or interruption here is new.
     - **No Working frame is required:** a fast reply can skip it.
     - **Reply:** pair it with the observer's `ordinary-reply` for the same prompt and completion. Its whole-line native and public readback decides the reply; the screen alone never does.
   - **J3 `refresh`:** one ordinary prompt and one `completion` line that only its reply produces.
     - **Prompt:** 1-100 characters, no padding or control character, no leading `/` or `!`, and no `$` or `@`.
     - **Completion:** at least 8 characters, and absent from the prompt, from `/skills` and from Codex's own text.
     - **Steps:**
       1. `refresh-prompt-composed`: the prompt drawn in the composer, not yet `Working`, and no completion on screen.
       2. Enter to `refresh-turn-running` (`Working`).
       3. `/skills` to `refresh-skills-command-ready`: its selected slash row, still `Working`.
       4. Enter to `refresh-skills-menu-open`: `Choose an action` with `› 1. List skills` selected.
       5. Down to `refresh-manage-selected`: `› 2. Enable/Disable Skills`.
       6. Enter to `refresh-manage-open`: `Turn skills on or off. Your changes are saved automatically.` and `space/enter toggle · esc close`, with no menu and no `No skills available.`.
       7. Esc to `refresh-manage-closed`: `Working` again, the view gone, an empty composer, and no `N skills enabled, M skills disabled`.
       8. No key to `refresh-reply-completed`: the completion, no `Working`, no interruption and an empty composer, within `reply_deadline_ms`.
     - **Every frame** forbids the completion and `■ Conversation interrupted` until the last.
     - **No `Working` on the view frames:** while a bottom-pane view is open Codex draws only the view (`bottom_pane/mod.rs:2418-2453`), so the menu and manage-view frames cannot show `Working`. Continuity comes from the frames around them and the exact key list.
     - **Esc only:** Space and Enter toggle a skill in the manage view, so the only key there is Esc, and Esc closes a view without interrupting (`bottom_pane/mod.rs:925-953`). Closing sends `ManageSkillsClosed`, which lists skills again with `forceReload` on the same app-server connection (`chatwidget/skills.rs:103-104`).
     - **No reload claims:** the screen shows neither that reload nor its result, so the journey claims no disk reload and no new provider skill.
     - **Setup:** the target setup that keeps the turn running through these steps is chosen per run. A Claude skill added mid-turn is a separate open gap.
   - **J3 `reopen`:** no input. The resume screen reaches `history-replayed`: every declared line of the reply this Session already holds. Only the visible screen is read, not scrollback, so declare the latest reply of a short Session. The run plan and the observer address the same raw Session and native alias.
   - Every journey ends with Codex's own exit: type `/quit`, wait for its slash-popup row (`exit Codex`), then Enter, then a bounded wait for PTY EOF. The Codex child must stay one process, and checkpoints must arrive in declared order.
4. Read back the same Session:
   `node verification/observations/acp-journey-observation.ts OBSERVER_PLAN.json`.
   Run it directly. Its own bounds cover every wait: `events observe` 20 s, WebSocket handshake 10 s, 15 s per response. Do not wrap it in `timeout`, which macOS lacks.
   - `idle-bind`, only after the TUI step is idle, checks `skills-listed`, `skill-effect`, `ordinary-reply`, `information-refused`, `compacted` / `compact-refused`, `effective-model` and `model-catalogue`. With `model-catalogue` it also reads native `model/list` before and after its own resume and the provider Session's own catalogue through public `session inspect`, keeping every raw response. The check requires every provider model in the bound list (only the declared delegation may be extra), a bootstrap list of the delegation alone, and the provider's current model as the native one. For `effort.kind: qualified` the selected model's native choices must equal the provider's effort options, its default must be an offered value, and the current values must agree. For `provider-settings` the row must carry exactly the declared action, which is never accepted as the effective effort. The native list is never its own reference.
   - `ordinary-reply` (idle-bind only) requires all of these:
     - the latest turn completed, holding exactly one user item that is the prompt as one Text input;
     - an assistant message with one line that is exactly the completion, apart from surrounding whitespace (the marker inside other prose, such as an error, never counts);
     - no earlier turn holding the completion;
     - `thread/read` and `thread/resume` agreeing on that turn;
     - the same whole line in the public provider events.

     A user item or composer echo never stands in for the reply. For `refresh`, pair it with `skills-listed` after the turn, and with an `active-read-only` `turn-in-progress` read during it.
   - **Claude route window** (`claude-route`, idle-bind only, paired with that plan's own `ordinary-reply` or `skill-effect`). A reply alone never shows the Host route, because normal home can answer through another credential.
     - **Run plan:** a Claude `prompt` or selected-skill plan may declare `claude_route`. It holds the normal debug Host's `host_run_directory`, its `host_pid`, `max_prefix_bytes` (at most 67108864), `max_segment_bytes` (at most 1048576) and `collaboration_cli`, the path and sha256 of the normal build's `agent-collaboration`. No other journey, provider or file is admitted.
     - **Binding:** `--check` and the baseline bind `<host_run_directory>/router-audit.jsonl` (automation_debug_host/launch_options.rs:257-273) to the declared Host:
       - an owner-private canonical run directory;
       - the Host's own `debug-host-context.json`, naming that run directory, Host PID, the target's service directory and a Claude provider;
       - its port equal to `service.json` `routerProxyEndpoint`;
       - the observer plan's `host_pid`;
       - the CLI's bytes, the observer plan's `collaboration_cli` and its build manifest's `agent-collaboration` entry, all naming that same path and sha256.
     - **Baseline:** the runner's fixed hook first takes the pre-Enter inventory read (below), then records the file's device, inode, byte length and prefix sha256 into `<run output>/claude-route-baseline.json`, with the target's service ID and epoch, the bound CLI and that read. It runs right after the `prompt-composed` or `skill-input-composed` checkpoint and its original capture, and refuses unless the next step's only action is the submission Enter. Model setup and readbacks must already be idle.
       - The Router creates the file lazily on its first decision, so the baseline may record `absent`. The whole file is then the window.
     - **Isolation:** the audit has no Session join, so the window counts only while the Host holds exactly the declared hosted Claude Session (provider_session_inventory.rs:163-414). The bound CLI reads it once at each boundary, never retried: `sessions list --endpoint claude-local --view stored --source all --any --page-size 100 --service-directory <declared> --json`. The pre-Enter read runs at the fixed hook before the audit baseline; the post-reply read runs after the reply readback. Stored records are never deleted, so a closed or unloaded Session still counts as another Session.
       - Each read passes only one complete page (`nextCursor` null, no `skippedRecords` or `nativeCoverage`) for the declared service, holding exactly one `hostedProvider` row for the exact raw target in state `idle`, with an RFC3339 UTC `observedAt` ending in `Z`.
       - stdout must be the CLI's one JSON document. Output size and run time are bounded; a failure is categorical and never carries CLI output.
       - `service.json` must hold the run target's service ID and epoch before and after each read, so a same-service restart fails, and the CLI's bytes must still hash to the bound sha256.
       - Retained per read: service, endpoint, Session, state, count and observation time. Never actor, policy or other row data.
     - **History:** `before_thread_read` names the earlier same-Session observer's retained raw `thread/read` response, idle at the exact alias and cwd. Both final `thread/read` and `thread/resume` views must keep its turns unchanged in full content (exact evidence serialization) and add exactly one completed turn: the one this plan's `ordinary-reply` or `skill-effect` established. Only the prior turn count and a digest are retained.
     - **Read:** declare the observer's `{kind: "claude-route", baseline: <that file>, before_thread_read: <that raw read>}`. After the reply readback, it requires the same binding, service instance, identity and unchanged prefix, then reads only the bounded appended segment.
       - Retained: categorical counts with `account_hash_present`, the segment's byte length and sha256, both inventory projections, the prior turn count and digest, and the baseline record. Never an account hash, request ID, header, body or free text.
     - **Pass:** at least one `claude_messages / valid / allowed / forwarded / committed` row with an account hash, and no rejected Claude row. Models/responses rows are another issuer and never count.
     - **Fail:** missing, malformed, replaced, truncated, prefix-changed or over-bound evidence, no forwarded Claude row, or a rejected Claude row. Also any other managed Session at either read, a wrong service, target, origin, state or time, pagination, skipped records, native coverage, a CLI error, malformed output or an unavailable CLI, a restarted service, a changed CLI or manifest binding, an extra same-Session turn, a changed prior turn, a wrong before-state, or a new turn that is not the reply turn. A failure never triggers a prompt retry or an auth diagnosis.
     - **Attribution:** it rests on the observed single idle managed Session at both reads, the single new reply turn and one private normal Host, because the audit has no timestamp, Session or prompt field and a constant request ID. A native Claude launch sharing the Host's private listener or token is invisible to the managed inventory, so keeping that listener and token unshared stays an allocation premise.
     - **Not success:** an allowed committed row is written for a forwarded response head even on an upstream error, so it is neither HTTP success nor stream completion. A valid-token request that fails account selection writes no row. The genuine reply and normal CLI quit stay mandatory; there is no audit-only pass.
   - `active-read-only`, during a running turn, checks only `turn-in-progress`. It never resumes or lists skills.

## Evidence and failures

Keep the run output and the observer evidence in the task's evidence home.

- **Run output:** `declared-plan.json`, `codex-target.json`, a `.grid.json` and an original PNG per checkpoint, `terminal.ansi`, `driver-commands.json`, `codex-acp-run.json` and `codex-acp-receipt.json`.
- **Observer evidence:** raw `app-server-*.json`, `provider-observation.json` and `result.json`, plus `claude-route-baseline.json` and the categorical `claude-route-window.json` (window, both inventory projections, prior turn count and digest) when the plan checks the route window. The run output holds the runner's `claude-route-baseline.json`.

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
- **Not yet scripted:** J3 approval. It needs a provider tool call in a permission mode that asks, and model input. Router forwards it as a native server request, and Codex's approval overlay answers it.
- **`refresh` setup:** it needs a turn that keeps running through the menu and Esc, and that turn's credentials. A provider command or skill that changes mid-turn remains unscripted: Cursor publishes its commands only at new or load.
- **Cleanup:** Codex exits through `/quit`, and the Rust entry requires child exit 0 and no survivors. Close the synthetic Sessions and stop the owned Host as [ACP metadata](acp-metadata.md) describes.
