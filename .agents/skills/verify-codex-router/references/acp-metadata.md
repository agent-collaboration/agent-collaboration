# ACP metadata in the installed Codex client

Use this journey when changing provider SessionItem projection or the Codex app-server face. Adopted outcome: the owner's ACP metadata repair requires useful metadata retained outside assistant prose, genuine transcript content preserved, and installed-client/provider proof. The same projection serves Claude, Cursor and other ACP providers; a passing provider run covers that declared provider, not every ACP implementation.

The entry is installed Codex `--remote` → candidate Host's `router-sessions/<endpoint>.sock` → actual installed ACP provider. `verification/observations/acp-metadata-observation.ts` independently reads the same Session through public `events observe`, `thread/read` and `thread/resume`. It never creates a Session, sends model input, logs in or stops a Host. The operator owns the actual installed-client drive and exact cleanup.

## Qualify the target

Use the existing isolated signed debug Host launch, normal Codex home and a declared unused debug port/root. `automation-debug-host --help` documents its fresh private runtime directory, explicit Router root and Claude/Cursor launch selectors. Its profile projection is in memory. Keep the production Host/Router, production sockets/settings and unrelated debug roots untouched. Never copy production credentials to make this journey pass.

Retain a committed source HEAD/tree, clean build state, compiler-artifact JSON/features, normal-build artifact hashes and signatures, exact owning Host PID/executable, endpoint/service/binding generation, installed Codex version/hash and actual global provider executable/version/hash. Use the normal-dependency `automation-debug-host` **bin**, not the example target (examples activate dev dependencies). A separate owned target directory keeps normal artifacts apart from Keychain-guarded test builds. The build manifest carries `source_commit`, `source_tree`, `dirty: false`, an empty `test_guard_features` list derived from the compiler records, and `binaries` keyed by `automation-debug-host` and `codex-router`, each with path/SHA256. Read the actual endpoint and Session settings before input. Use advertised model/mode/effort values; preserve unrelated companion settings. `notReported` auth is not logged out. Claude's native login status does not establish its isolated Router pool credentials: the Host supplies that pool's endpoint and token. An unusable pool requires an authorized isolated login before its reply control can pass.

Create one synthetic provider Session through `agent-collaboration conversation create` with the caller's authorized actor/access/cwd and `--approver-owner` for this local-owner Codex TUI journey, then inspect it using `session inspect`. The TUI sends settings actions as the local owner; an agent-created Session whose Approver defaults to that agent can reject the TUI's model selection before model input. Keep the requested access restricted and do not change an existing Session's authority. Retain the genuine returned target and operation ID. Use its UUID for the Codex resume below. Do not fabricate a Session or read provider transcript files.

## Drive the installed client and replay

On macOS, capture the real client with the system `script` command (other platforms need their own qualified PTY capture). Use the exact installed executable paths, private service directory and trusted worktree you declared:

```sh
script -q "$EVIDENCE/live.pty" "$CODEX" --no-alt-screen \
  --remote "unix://$SERVICE_DIRECTORY/router-sessions/$ENDPOINT_ID.sock" \
  --cd "$WORKTREE" -c features.hooks=false resume "$SESSION_ID" "$PROMPT"
```

Make the no-tools prompt ask for words joined by underscores, for example “Reply with exactly three uppercase words joined by underscores: CURSOR, ACP, OK. Do not use tools, inspect files, or perform any other work.” The expected reply `CURSOR_ACP_OK` is absent from the literal prompt, so terminal presence cannot be explained by input echo alone. Choose a fresh marker for each new model input. Wait for the actual reply and completed Turn, then exit only this TUI using its normal keyboard shutdown.

Reopen that exact Session in another captured installed Codex TUI, without a prompt or new model input:

```sh
script -q "$EVIDENCE/resume.pty" "$CODEX" --no-alt-screen \
  --remote "unix://$SERVICE_DIRECTORY/router-sessions/$ENDPOINT_ID.sock" \
  --cd "$WORKTREE" -c features.hooks=false resume "$SESSION_ID"
```

Require the ordinary reply in both captures and no raw command-catalogue JSON. Observe the history, then exit this TUI normally. A launch, idle prompt or provider authentication alone proves no rendering behavior.

## Read back the retained metadata

Read `MetadataObservationPlan` in `verification/observations/acp-metadata-observation.ts`. Save its JSON in the task evidence with repository/source and `build_manifest`, candidate Host/Router hashes and Host PID, installed Codex path/hash, installed collaboration CLI, private service directory, exact endpoint/Session ID, both original PTY paths, expected reply and a fresh evidence output directory. The Host must still be running and the committed worktree clean. Set `require_usage` only when this run is required to demonstrate actual usage emission.

```sh
node verification/observations/acp-metadata-observation.ts "$METADATA_PLAN"
```

Passing evidence includes original provider metadata items with their IDs/kinds/exact payloads, genuine assistant output in those events, the same output in both Codex API views and actual live/reopened PTYs, and no metadata item IDs in either Codex transcript. The script retains raw API responses before reducing the result. An absent catalogue, truncated/reset observation, wrong target/build or missing ordinary reply fails. A provider that emits no Usage leaves usage/context/cost runtime proof explicitly unobserved; permanent ACP live/replay regressions cover those payloads separately and cannot be relabelled real-provider observations.

Metadata is retained under the existing loaded-Session observation contract, not a new durable snapshot: unload/close/provider loss evicts retained items, epoch changes require reattachment, and event/byte/deadline bounds apply. Capture the observation before closing the synthetic Session. Do not add native token counters or label provider commands as Codex filesystem skills to manufacture a UI surface.

## Apparatus and cleanup

```sh
pnpm --dir verification run test:observations
```

Run `check`, `lint` and `format:check` through the verification package. The independent oracle cases reject metadata leakage in either history view, metadata deletion, lost genuine prose, wrong threads, incomplete observation, missing PTY rendering and silently missing required usage. They establish apparatus sensitivity, not product proof.

After collecting the exact observation, close only the synthetic provider Session if its runtime advertises close. When close is unsupported, retain the typed rejection/operation ID and stop the owned runtime; do not invent a close, delete vendor history or retry the rejected operation. Preserve uncertain operation IDs. Stop only the recorded task-owned Host/process closure using the existing identity guards. Verify protected production identities/settings, no owned survivors and surviving evidence. Return per-provider observations and gaps, exact source/build/tools, real IDs, quality commands/exits and cleanup. The journey remains unqualified for a provider until its actual installed route is observed; do not infer Claude from Cursor or Cursor from Claude.
