# TypeScript PTY support

These added files support the smaller real-PTY foundation. The node-pty workaround was renumbered UW-8 when #173 already owned UW-6 and UW-7. The CI workflow was already counted in the historical inventory.


## pty-process.ts

- **Path:** [verification/terminal/pty-process.ts](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/terminal/pty-process.ts)

- **Now:** Changed in surviving branch.

- **Purpose now:** Controls one real OS PTY child: input, resize, bounded raw output, exit and child-only cleanup.

- **Why this change remains (source-based explanation):** Supports the smaller real-PTY foundation and its checks; it does not establish menu behavior.

## pty-process.test.ts

- **Path:** [verification/terminal/pty-process.test.ts](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/terminal/pty-process.test.ts)

- **Now:** Changed in surviving branch.

- **Purpose now:** Contains the 13 permanent PTY/entry tests, including wrong-program and alternate-Codex-home refusal.

- **Why this change remains (source-based explanation):** Supports the smaller real-PTY foundation and its checks; it does not establish menu behavior.

## codex-pty.ts

- **Path:** [verification/terminal/codex-pty.ts](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/terminal/codex-pty.ts)

- **Now:** Changed in surviving branch.

- **Purpose now:** Runs only Codex version/help through a real PTY and records executable/output identity and named config stability.

- **Why this change remains (source-based explanation):** Supports the smaller real-PTY foundation and its checks; it does not establish menu behavior.

## package.json

- **Path:** [verification/package.json](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/package.json)

- **Now:** Changed in surviving branch.

- **Purpose now:** Pins node-pty and exposes focused/aggregate terminal verification commands.

- **Why this change remains (source-based explanation):** Supports the smaller real-PTY foundation and its checks; it does not establish menu behavior.

## pnpm-lock.yaml

- **Path:** [verification/pnpm-lock.yaml](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/pnpm-lock.yaml)

- **Now:** Changed in surviving branch.

- **Purpose now:** Locks the TypeScript/native PTY dependency resolution.

- **Why this change remains (source-based explanation):** Supports the smaller real-PTY foundation and its checks; it does not establish menu behavior.

## pnpm-workspace.yaml

- **Path:** [verification/pnpm-workspace.yaml](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/verification/pnpm-workspace.yaml)

- **Now:** Changed in surviving branch.

- **Purpose now:** Permits the pinned native PTY dependency build script.

- **Why this change remains (source-based explanation):** Supports the smaller real-PTY foundation and its checks; it does not establish menu behavior.

## uw-6-node-pty-macos-spawn-helper.md

- **Path:** [docs/upstream-workarounds/uw-6-node-pty-macos-spawn-helper.md](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/docs/upstream-workarounds/uw-8-node-pty-macos-spawn-helper.md)

- **Now:** Moved in base or renumbered; current home: `docs/upstream-workarounds/uw-8-node-pty-macos-spawn-helper.md`.

- **Purpose now:** Documents the observed macOS spawn-helper permission defect, supported source-build route and removal condition; now UW-8.

## README.md

- **Path:** [docs/upstream-workarounds/README.md](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/docs/upstream-workarounds/README.md)

- **Now:** Changed in surviving branch.

- **Purpose now:** Adds the node-pty workaround to the shared upstream-issue index.

- **Why this change remains (source-based explanation):** Keeps the dependency defect and its removal condition inspectable.
