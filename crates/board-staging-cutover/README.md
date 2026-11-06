# One-time project cutover

This helper carries explicitly inventoried native projects to an explicitly
selected Central. It preserves the existing sealed bundle, current-source
admission, authenticated publication, whole-directory backup, ordinary Claim,
cold content verification and public canary paths. It never stops or starts a
Router Host.

## Production operation

Production requires the exact OS-account home-default `.codex-router` root,
`--production`, an explicit `--target` matching the preserved plan, and the
digest from a fresh dry run. Standing `.codex` and `.codex-router-debug` roots
remain refused. Isolated staging commands retain their existing syntax.

Inspect the production plan without authentication, network access, a writable
database or a Host lock artifact:

```sh
cargo run -p board-staging-cutover -- offline --plan "$PLAN" \
  --production --target "$TARGET" --dry-run
```

The JSON report lists every selected project and file epoch, its bundle and
backup paths, the target URL, and `digest`. The digest binds the full validated
plan, canonical Host identity, root device/inode, project inventory and bytes,
and existing bundle/backup identities and bytes. Freshness means that these
inputs still match; there is no clock-based expiry or persisted dry-run receipt.
A live Host may change these inputs after inspection, requiring a new dry run
after the owner stops it.

After the owner has stopped the Host, confirm the reviewed digest:

```sh
cargo run -p board-staging-cutover -- offline --plan "$PLAN" \
  --production --target "$TARGET" --confirm-production "$DIGEST" \
  --owner-credential-file "$OWNER_HANDOFF"
```

Confirmation is checked before any lock artifact or credential consumption,
then again while holding the existing Host singleton lock before export or
publication. A running Host excludes the helper. The target is never inferred
from enrollment or credentials. The existing `staging-cutover-backups` directory
name and sealed bundle format are retained for both modes.

The existing phase split still applies: `offline` prepares, publishes and backs
up; `claim` uses the enrolled running Host; `verify` holds the offline Host lock;
`canary` writes and reads back through the running Host. Use a fresh dry run and
the same production flags for each phase. `offline` and `claim` require
`--owner-credential-file` or `--owner-access`; a dry run requires neither.
Cold verification precedes the canary, since that public write changes content.
After restarting for the canary, replay the same frozen Claim to attach the writer.
A physical e2 writer may still expose its unchanged e1 application-history head;
the canary writes at the next position in e2 and reads back through the retained
predecessor cursor. Wrong-epoch authority, foreign projects and stale views refuse.
A nonzero phase result remains incomplete or unverified.

## Isolated production rehearsal

Debug builds alone compile the `CODEX_ROUTER_CUTOVER_TEST_HOME` override. It must
point to an existing absolute, owner-private fixture home, distinct from the
OS-account home and outside its protected Router/Codex roots. Put the fake
production root at `<fixture-home>/.codex-router`. This permits the same compiled
commands to rehearse against TestCentral or a separately authorized staging
target. The override is under `#[cfg(debug_assertions)]`; release builds ignore
it and use `getpwuid`. Changing `HOME` alone never selects Host authority.
The debug override also refuses the real OS-account protected roots.

The permanent tests and their proof boundaries are described in
[tests/README.md](tests/README.md). They never operate on the real production
root or replace a production process. Hosted staging observation is separate
from the local TestCentral proofs.
