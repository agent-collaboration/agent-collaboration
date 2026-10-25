# Quota and Sessions capture tools

Run these tools with Node 26.12.1 and the package-local pnpm 12.14.0 toolchain.
Node executes the TypeScript sources directly. Plans bind the absolute Node
executable and its SHA-256 alongside the driver, fixture, VHS, ttyd and image
checker. The parent CLI must use the declared executable.

From the repository root:

```sh
pnpm --dir scripts/verification/tui install --frozen-lockfile
pnpm --dir scripts/verification/tui typecheck
pnpm --dir scripts/verification/tui lint
pnpm --dir scripts/verification/tui format:check
pnpm --dir scripts/verification/tui test
node scripts/verification/tui/capture-run.ts --help
```

The permanent Rust adapter tests also run in ordinary workspace test runs. Before
running them locally, install the locked package dependencies as above and export
the selected Node runtime's canonical path and digest:

```sh
export QUOTA_STORYBOARD_NODE="$(node -p 'require("node:fs").realpathSync(process.execPath)')"
export QUOTA_STORYBOARD_NODE_SHA256="$("$QUOTA_STORYBOARD_NODE" --input-type=module -e 'import { createHash } from "node:crypto"; import { readFileSync } from "node:fs"; process.stdout.write(createHash("sha256").update(readFileSync(process.execPath)).digest("hex"));')"
```

The tests require Node 26.12.1 and reject a different executable or digest. These
variables nominate the runtime; they do not start a capture or install tools.
The CI TypeScript job and Rust test job use the same prerequisites. Capture plans
still carry their own exact runtime binding.

`capture-run.ts` runs a declared compact capture or bounded live CUA lease.
`component-capture-run.ts` runs a declared Normal or hard-case Quota capture.
`evidence-readback.ts` records source bindings and same-child PTY dimensions.
`public-sessions-run.ts` qualifies the existing declared Host read-only before
launching the public Sessions entry. It preserves normal home directories and
requires separate effect and job-settlement evidence.

The native Rust harness owns the PTY and capture resources. Its permanent
`fixture-child.ts` and `ttyd-adapter.ts` helpers inherit the native descriptors;
the TypeScript core does not emulate terminal rendering or allocate another PTY.

Tests use isolated Git repositories, controlled child processes, protocol-only
image bytes and a controlled HTTP listener. These validate rejection, lifecycle
and binding behavior. They do not establish product appearance, provider behavior
or delivery acceptance. Galleries retain pending personal image inspection and
explicit unverified rows.

Discovery and image-checker subprocesses retain private raw output files in
system temporary directories. A deadline never becomes success after later exit.
Capture cancellation uses the declared cooperative marker; an unsettled driver
is reported without broad process signaling or speculative cleanup.
