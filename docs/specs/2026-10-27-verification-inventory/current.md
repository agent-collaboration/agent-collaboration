# What remains, and why

**Keep a small terminal helper and product regressions. Build one real menu journey when the client/provider boundary is settled.** No new verifier framework is proposed here.

## The three TypeScript files

| File | Job | Limit |
| --- | --- | --- |
| [pty-process.ts](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/terminal/pty-process.ts) | Start one OS PTY child; send input; resize; retain bounded raw output; observe exit; stop its own child | Does not render a screen or know skills, plugins or ACP |
| [pty-process.test.ts](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/terminal/pty-process.test.ts) | Exercise real terminal I/O, child replies, resize, exit, stale output, overflow and invalid launches | Passing these tests proves driver behavior, not the feature |
| [codex-pty.ts](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/terminal/codex-pty.ts) | Run the installed Codex executable's version/help under the PTY; record executable/output hashes and unchanged named config files | Never opens a conversation, invokes a model or tests a menu |

The test file declares 13 tests; all 13 passed within the 848-test Sunclaw verification run described below. No xterm/headless renderer is used.

The pinned `node-pty` dependency needs its supported source-build route on the observed macOS setup because the packaged spawn-helper was not executable. [UW-8](../../upstream-workarounds/uw-8-node-pty-macos-spawn-helper.md) keeps that evidence and removal condition. Package import alone does not prove a PTY can spawn.

## Product regressions that remain

**Submitted input:** the real ACP SDK and session actor are exercised against a scripted provider. The tests check that admitted user text survives even when the provider supplies no echo, with native user groups kept separate. Rejection and sink-failure cases check the boundary. This earns its place as a Router regression; it is not a Claude or Cursor run.

**Metadata:** Host projection tests feed actual ACP decoding with scripted metadata, then check live/history presentation. They retain useful command data while keeping it out of assistant prose. They do not observe the installed client menu.

**Validation repairs:** board listener identity, mail-notice recovery, WebSocket fixture cleanup, process-fixture exit and public-observation diagnostics remain in their owning Rust tests. They address the required validation path; they do not implement skills or slash commands. The [complete path inventory](files.html) makes these visible separately from menu work.

**Legacy SQLite fixtures:** two tests now copy committed databases into their own temporary directory before opening them. That prevents concurrent readers of the same committed fixture from tripping the existing open-file guard. The production guard stays intact.

## What the real menu check still needs

The [provider-menu Verification Design](../2026-10-26-acp-provider-menus/proof.html) owns those checks. It separates a real selection and provider effect from visible membership in the actual terminal window, then covers updates, stale input and history. The PTY helper supplies terminal I/O for that future journey; its current version/help smoke cannot stand in for it.

Reuse existing target allocation, Router recording and process helpers. The feature journey is still missing; the retained version/help driver is only the terminal foundation.

## Current proof status

Observed on Sunclaw in the isolated checkout at product source `424fbc238`, 2026-10-27:

| Check | Observed result | What it establishes |
| --- | --- | --- |
| `pnpm --dir verification` install, check, lint, format:check | Each exited 0; lint reported zero warnings/errors | Dependency install and TypeScript static gates |
| Supported `build:pty`, then `pnpm --dir verification test` | Both exited 0; 848 passed, zero failed/skipped, including all 13 PTY cases | Harness behavior and scripted regression cases on this Mac |
| `cargo check --locked -p acp-client-runtime --tests` | Exit 0; one existing unused-import warning | This package and its test targets compile |
| `cargo test --locked -p acp-client-runtime submitted_prompt_tests` | Exit 0; four passed, zero failed | Actual actor/SDK behavior with the scripted provider |
| `cargo check --locked -p codex-router-host --tests` | Exit 0; cached dependency-template diagnostics appeared before compilation | Host and its test targets compile after the #173 crate move |

The full Rust workspace/lint gates and new menus remain unverified. These checks do not run real Claude/Cursor conversations or observe the actual terminal menus. The four old test-helper crate qualifiers found during this inventory were corrected to `collaboration-contracts`; the Sunclaw Host check now validates that correction compiles. It does not execute those Host tests. Sunclaw currently has no usable Developer ID signing identity for a new debug Router; no new Router was launched by these checks.

The Host log begins with diagnostics about unrendered `{{project-name}}` and template conditionals in cached `workers-rs` manifests. Cargo then checks the declared Host targets and exits 0. The origin of those non-fatal diagnostics has not been established here; the run is not described as diagnostic-free.
