# Stopped Router root conversion

Use the repository's pinned Node version:

```sh
node scripts/one-off/2026-11-convert-router-root.ts --root /absolute/router-root --dry-run
node scripts/one-off/2026-11-convert-router-root.ts --root /absolute/router-root
```

Omitting `--root` selects `~/.codex-router`. The script never stops a process.
Run it only after the Host exits. A present Host socket, an open singleton artifact,
symbolic link, special file or conflicting proxy filename refuses before any
live-root write. Resolve the named path before retrying.

Apply creates a dated sibling backup and checks every regular file's bytes and
permission bits before converting. Only the provider proxy store carries over:
`state.sqlite` is checkpointed and renamed to `provider-proxy.sqlite`, with its
remaining WAL/SHM files and credential-lock directory.

Every other path stays only in the verified backup, including settings, the machine
list, approval routes, interactions, identities, secrets and collaboration files.
The Router starts with its defaults. Dry run and the conversion summary list the
backup-only paths without printing their contents. No old board schema is imported.

Preflight refusals leave the live root unchanged. Apply failures can follow a
completed step; they name that step, keep the verified backup and print a restore
command that moves the incomplete root aside before copying the backup back.
Run a restore only with the Host stopped. The script has no journal or rollback
engine. It is not run automatically by installation or release tooling.

The permanent cases live beside the script and run through `pnpm --dir verification
test` via the package collector. Fixture provenance is in [fixtures](fixtures/README.md).
