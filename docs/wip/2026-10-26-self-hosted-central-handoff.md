# Run your own central and import existing boards

The release containing this change runs central inside `codex-router`, with local durable storage and its own RS256 issuer. Cloudflare is not required. The existing Cloudflare deployment and enrollment commands keep working.

Install that release with Homebrew:

```sh
brew update && brew upgrade codex-router
codex-router central serve --help
```

Set these paths in the terminals used below. `CENTRAL_ROOT` is central's own private directory. `ROUTER_ROOT` belongs to the machine joining it.

```sh
export CENTRAL_ROOT="$HOME/.codex-router-central"
export CENTRAL_URL="http://127.0.0.1:19843"
export ROUTER_ROOT="$HOME/.codex-router"
```

Run central in its own terminal:

```sh
codex-router central serve --root "$CENTRAL_ROOT" --listen 127.0.0.1:19843
```

Leave it running. Ctrl-C or SIGTERM drains the server; start the same command on the same root to resume. Its signing key uses Router's Keychain-protected secret store. The public keys are served at `/.well-known/jwks.json`.

In another terminal, mint and enroll. These commands hand the credential through the local secret store and print no token:

```sh
codex-router central mint --root "$CENTRAL_ROOT"
codex-router fleet enroll \
  --endpoint "$CENTRAL_URL/v1/session" --label "local-machine" \
  --self-hosted-root "$CENTRAL_ROOT" --router-root "$ROUTER_ROOT"
codex-router fleet status --router-root "$ROUTER_ROOT"
```

The machine credential lasts one year by default; status reports its expiry. Renew it by repeating the same mint and enroll commands, then starting the Router again. Enrollment takes effect at the next Router start and never restarts it automatically.

Before importing, back up the legacy stores and stop the Router being migrated through its normal owner procedure. Set `LEGACY_SOURCE` to the closed legacy board database and `IMPORT_REPORT` to a new report path outside the source files. Rehearse on a copy first.

```sh
codex-router-board-upgrade \
  --source "$LEGACY_SOURCE" --router-directory "$ROUTER_ROOT" \
  --report "${IMPORT_REPORT}.dry-run.json" --dry-run

codex-router central mint --root "$CENTRAL_ROOT" --kind owner
codex-router-board-upgrade \
  --source "$LEGACY_SOURCE" --router-directory "$ROUTER_ROOT" \
  --report "$IMPORT_REPORT" --central-url "$CENTRAL_URL" \
  --self-hosted-root "$CENTRAL_ROOT"
```

Owner credentials last 15 minutes. Mint immediately before importing; mint again if an owner credential expires. The importer reports what it uploaded and preserves the legacy board source.

Start the enrolled Router in its own terminal, then claim the imported projects using their IDs from the report:

```sh
codex-router host --router-root "$ROUTER_ROOT"
```

```sh
agent-collaboration project claim "$PROJECT_ID" \
  --service-directory "$ROUTER_ROOT/agent-communication" --json
```

Repeat the claim for each imported project that is open. Existing archived-board restrictions still apply. Use the normal board commands after a successful claim.

For a private network, set `CENTRAL_ADDRESS` to the central machine's reachable address and start central with:

```sh
codex-router central serve --root "$CENTRAL_ROOT" \
  --listen "$CENTRAL_ADDRESS:19843" --public-url "http://$CENTRAL_ADDRESS:19843"
```

Choose this public URL on the first start; it becomes the saved issuer identity. Set `CENTRAL_URL` to that URL on each client. On the central machine, export a credential for a different machine:

```sh
codex-router central mint --root "$CENTRAL_ROOT" --out "$ENROLLMENT_HANDOFF"
```

Transfer the file privately, keeping mode `0600`, then use `--enrollment-credential-file "$ENROLLMENT_HANDOFF"` instead of `--self-hosted-root` in `fleet enroll`. The receiving command consumes and removes the file. For an importer on another machine, mint with `--kind owner --out "$OWNER_HANDOFF"` and use `--owner-credential-file "$OWNER_HANDOFF"` instead of `--self-hosted-root`.

For an isolated debug rehearsal only, add `--allow-plaintext-file-secrets` to `central serve` and `fleet enroll`, with separate scratch roots and ports. Release builds do not offer this option.
