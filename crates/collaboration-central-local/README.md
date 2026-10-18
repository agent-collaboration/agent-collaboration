# Local central

A native axum shell over `collaboration-central-core`, for development and the
shared conformance suite. Each object owns one Turso 0.8.1 file and one serial
blocking actor. Each core step runs inside `BEGIN IMMEDIATE`; every error gets
an explicit `ROLLBACK`, and effects are carried only after commit.

The shell verifies `Cf-Access-Jwt-Assertion` before routing or parsing a request
body. Machine sessions additionally require the matching installation's machine
credential in `Authorization`; enrollment stores its Access client ID and
display label. Admin
requests require the configured owner's identity from the admin application's
signed assertion. There is no admin bearer secret.

The executable takes `--access-team-domain`, `--access-session-audience`,
`--access-admin-audience`, `--access-owner-identity` and `--access-jwks-url`,
along with `--root` and `--listen`. Its configured JWKS source lets local tests
serve a real HTTP key endpoint. Native signature checks use RSA/SHA-256; all
claim, app, key selection and caller rules are shared with the Worker core.

Admin routes are `POST`:

- `/admin/v1/machines/enroll`: an `AdminRequest::EnrollMachine` JSON body.
- `/admin/v1/machines/{installation}/revoke`.
- `/admin/v1/import/projects/{project}`: a `ProjectImportPage` body.
- `/admin/v1/projects/{project}/takeover/preview`.
- `/admin/v1/projects/{project}/takeover`: an `AdminRequest::Takeover` body.
- `/admin/v1/projects/{project}/move`: an `AdminRequest::Move` body.

Body project IDs must agree with the path. The core owns enrollment, transfers,
leases and every response. A newer Hello replaces the old physical connection;
correlation and replay state survive actor eviction and server restart. Platform
alarm records live in `platform-alarms.json` alongside the object files and are
restored on startup. Shutdown cancels and joins socket work, timers and actors.

Run real-engine and real-WebSocket conformance:

```sh
cargo test -p collaboration-central-conformance
```

The `conformance` feature adds an owner-assertion-authenticated `/__conformance/control` route
and fault controls. It is disabled in the default executable. Shared scenarios
and the test-control contract live in `collaboration-central-conformance` for
reuse by the Cloudflare test shell. `SessionResumeScenario::prepare` and
`verify` split the same resume proof across a platform restart.

Forwarded posts use the same authenticated session. Central routes opaque
`BoardPayload` envelopes to the current holder and correlates its answer to the
sender. It stores only correlation metadata, reserves at most 16 pending
forwards per sender and expires them after 30 seconds. The shared suite uses
a fixed holder reply as the named M4 admission stand-in; it does not establish
the holder's real board admission behavior. Archived project copies continue
applying fenced pieces so readers retain an up-to-date copy.

Owner imports stage pages in index order. A non-final page returns
`Staged{digest,index}`; only a validated final page publishes a readable copy.
The copy starts unheld at epoch 1. A machine's `Claim{attempt,project}` fences
it at the next epoch before returning `Claimed{epoch,expires_at,start}`.
Repeating an attempt returns its original decision. An existing holder makes
another claim return `AlreadyHeld`; moves and takeovers still need the owner.

The shared `ImportDigestBuilder` in `collaboration-replication-protocol` hashes
the logical header, rows in table/primary-key order and final summary, using
typed JSON items with eight-byte big-endian length prefixes. Page boundaries,
cursors and freshness do not affect identity. An identical import returns
`AlreadyImported`, including after a claim. Other content returns
`ImportConflict`, with the original import digest or `NotImported` for a
holder-created copy. The copy and its summary stay unchanged.

The conformance build has an explicitly test-only Fleet receiver for directory
producer acknowledgements and retry. It records update messages, without an
entry store or projection. Real directory entries, receiver integration and
`/admin/v1/import/fleet` remain M3.5 work.

M3.4 fixtures stand in for accepted Fleet admission using the named registry
reservation boundary. Fleet metadata, archive, notice cursors, tail reports and
ADR admission are M3.5. The capacity fault exercises typed refusal and rollback
on real Turso, but does not prove actual disk or Cloudflare quota exhaustion.
Local proof does not establish Durable Object equivalence or deployed behavior.
