# UW-8: node-pty 1.1.0 macOS helper permissions

**Status:** in place, 2026-10-26
**Library:** node-pty 1.1.0

The real-terminal verifier uses `node-pty` in `verification/`. On the observed
Apple Silicon macOS/Node 26.11.1 target, its published 1.1.0 package could load
the native addon but could not spawn a PTY child: `posix_spawnp failed.`

The exact [published package](https://registry.npmjs.org/node-pty/-/node-pty-1.1.0.tgz)
contains `prebuilds/darwin-arm64/spawn-helper` with mode `0644`. It was `0600`
after installation under the private umask; neither mode is executable. The
package's Unix implementation passes this helper to its macOS spawn path.
All eight initial real-PTY cases failed at spawn, before reaching their intended
behavior. They were setup failures, not failing Codex or provider tests.

Use the package's supported source-build route after installing dependencies:

```sh
pnpm --dir verification install --frozen-lockfile
npm_config_build_from_source=true pnpm --dir verification rebuild node-pty
pnpm --dir verification test:terminal
```

The observed rebuild exited 0 and produced an executable
`build/Release/spawn-helper` through the normal linker, without manually changing
cached package permissions or patching the PTY implementation. Node-gyp requires
its normal compiler/Python build tools; no Python source is added to this repo.
The build retained libc++ deployment-target and initializer warnings, so its
success must not be described as warning-free. The real-PTY test result is a
separate gate from successful compilation. Other OS/architecture combinations
need their own run; this record does not qualify them.

Remove this source-build prerequisite when a replacement pinned package passes
the real-PTY suite after a fresh normal install, with an executable helper and
without this override. A package version bump or successful import alone does
not establish that removal condition.
