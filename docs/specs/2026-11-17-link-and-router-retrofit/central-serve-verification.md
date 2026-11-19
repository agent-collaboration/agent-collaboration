# Launch an isolated Tokio central

Use a fresh directory and a test sign-in issuer that publishes its JWKS. Keep the
configuration, signed fixtures, and issuer responses in the verifier's private
directory. The issuer's audience and origin values must match the central
configuration exactly.

## Launch

Create a JSONC file with these settings, substituting the verifier's absolute
root and the issuer's public settings.

```jsonc
{
  "listen": "127.0.0.1:19843",
  "root": "/private/tmp/central-link-proof/central",
  "signIn": {
    "issuer": "https://issuer.example.invalid",
    "audience": "central-stamp",
    "adminAudience": "central-admin",
    "machineAudience": "https://central.example.invalid/machine",
    "publicOrigin": "https://central.example.invalid",
    "jwksUrl": "https://issuer.example.invalid/jwks",
    "serviceId": "019f0000-0000-7000-8000-000000000001"
  }
}
```

Run the signed debug CLI from the repository root and keep it in the foreground.

```sh
cargo run --locked -p codex-router-cli -- central serve --config "$CENTRAL_CONFIG"
```

The startup line gives the actual TCP address and the private socket. The TCP
listener serves `/v1/organizations/{organizationId}/link`. The private
listener is `$CENTRAL_ROOT/signin.sock` with mode 0600. Its root must be owned
by the current account and have mode 0700. Use SIGINT or SIGTERM to drain both
listeners and remove the socket.

## Seed the organization and enroll a machine

Use the test issuer's signed organization statement in `provision.json` as
`{"organizationId": "...", "organizationStatement": "..."}`. The issuer's
current device-login contract uses `installation` for the Router's UUIDv7 and
also carries `peer` until the Switchboard vectors are replaced. Use the same
issuer contract to write `device-login.json`. Use the returned enrollment ID
in `machine-token.json` with the organization and client IDs.

```sh
umask 077
curl --fail-with-body --silent --show-error --unix-socket "$CENTRAL_ROOT/signin.sock" \
  --header 'Content-Type: application/json' --data-binary @"$FIXTURES/provision.json" \
  http://localhost/internal/organization-provisioning > "$FIXTURES/provision-reply.json"

curl --fail-with-body --silent --show-error --unix-socket "$CENTRAL_ROOT/signin.sock" \
  --header 'Content-Type: application/json' --data-binary @"$FIXTURES/device-login.json" \
  http://localhost/internal/device-login-decisions > "$FIXTURES/device-login-reply.json"

curl --fail-with-body --silent --show-error --unix-socket "$CENTRAL_ROOT/signin.sock" \
  --header 'Content-Type: application/json' --data-binary @"$FIXTURES/machine-token.json" \
  http://localhost/internal/machine-token-decisions > "$FIXTURES/machine-token-reply.json"
```

Inspect the decision variants for successful provisioning, approval, and grant.
The token decision is a grant for the issuer to sign. Supply that signed token to
the Router's link fixture and require a Welcome carrying the same machine ID.
The existing Tokio link fixture builds, signs, and uses these requests.
[Tokio link fixture](../../../crates/collaboration-central-tokio/tests/integration/workspace_link/fixture.rs)

## Inspect a request receipt

After a semantic write, use its UUIDv7 request ID to inspect the owning database
read-only. Organization directory and mail writes use the organization file.
Workspace writes use the named workspace file.

```sh
sqlite3 -readonly "$CENTRAL_ROOT/central/$ORGANIZATION_ID.sqlite" \
  'SELECT request_id, caller, created_at FROM runtime_request_log ORDER BY created_at;'

sqlite3 -readonly "$CENTRAL_ROOT/workspace-stores/$WORKSPACE_ID.sqlite" \
  'SELECT request_id, caller, created_at FROM workspace_request_log ORDER BY created_at;'
```

Match the request ID and caller to the actual link reply. A retry of the same
write should keep one logged decision in the same store.
