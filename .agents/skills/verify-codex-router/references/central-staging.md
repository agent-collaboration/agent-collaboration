# Central staging: G4 / OB7

The replacement entry is `scripts.verification.central_staging`, commissioned
by decision 1098. Its four groups are qualified on local workerd with two real Routers;
deployed staging remains unobserved. Adopted sources are board
Verification Design OB7 and §5c–§5d, and one-core Program Design §13. Preparation
and local workerd do not close deployed staging obligations.

## Declaration and execution

Use an owner-private JSON declaration outside the checkout. Checked inputs are
in `scripts/verification/central_staging_models.py`. Pin a full source SHA,
actual Router binaries, native reqwest probe and Worker bundle hashes. Private
evidence and runtime directories must be fresh and isolated. Credential fields
carry environment **names**, never values.

```sh
uv run --offline --no-project --with pydantic==2.14.1 --python 3.13 \
  python -m scripts.verification.central_staging --plan "$PRIVATE_PLAN"
```

Default preparation resolves no credential, launches nothing, makes no request
and records OB7 unobserved. A `localWorkerd` declaration selects
`--execute-local` only after a local build/runtime grant. It owns an isolated
real workerd, fixture JWKS and two actual model-free Router Hosts. Synthetic
enrollment is local only. Five public SQLite bindings and the four required
settings use production durations (decision 1115).

A `staging` declaration selects `--execute-staging` only after a separate live
target/deploy grant. It pins script, deployed version/source, dedicated HTTPS
origin, Access policy identity, complete alternate-address inventory, owner
token environment, receiver/relay identities and two already-enrolled stopped
fixture installations. No staging enrollment operation exists in this driver.
It copies only declared private stopped fixture roots into owned system-temp
roots and verifies actual installation and peer identities before attaching
credentials. Normal account roots and ports 8787, 8788 and 43127 are excluded.

Deploy cases also pin the exact guarded deploy executable, arguments, working
directory, source/configuration hashes and namespace pin. Private fleet-ops
owns login, first creation, cutover, rollback and inventory. This driver never
calls cf or discovers Cloudflare credentials itself.

## Selected groups

- `access`: owner operations read through the real verifier; actual Router
  reqwest upgrade with A's Access identity and B's machine bearer preserves
  WrongKind; headerless 401/403 maps to Unknown; machine credentials fail on
  admin. Staging additionally checks missing/invalid Access and both session
  and admin paths at every independently inventoried alternate public origin,
  without following redirects. Two actual Router sessions and copied data
  supply the positive for header transit.
- `replication-deploy`: hold an actual Connection-to-Copy dispatch, commit an
  immutable post, observe the hold, deploy/reopen, and require the exact post
  once in the other Router's read copy. Both persisted session IDs must survive;
  exact retry preserves the decision and one feed effect.
- `takeover-recovery`: lose the Copy fence reply during owner takeover, read
  the advanced Copy fence and unchanged acknowledged prefix before deploy,
  replay the same transfer afterward, and require one exact next-epoch grant,
  original prefix, exact answer replay and target peer in owning Copy health.
  Wait for the successor to install the writer, then commit a real post there
  and read its exact bytes and enrolled origin through the former holder’s Copy
  before draining either Router.
- `idle-resume`: retain session IDs and Copy fence, wait the declared interval,
  then replicate a new post through both actual Routers. This proves survival;
  physical eviction still needs an existing platform construction signal.

Locally, the existing owner-gated verifier control receives valid signed bytes
and then a signature-byte forgery. The actual Worker must accept Owner and
return BadSignature respectively. Decision 1115 forbids a new injection route
or claiming this forged assertion passed through deployed Access. The local
Access fixture is a named stand-in. Headerless local HTTP is only an adapter
component fixture; it never proves the deployed edge's behavior.

## Qualification and limits

The retained local qualification at product source `600aadd3` covers all four
groups, clean shutdown, a real process-restart negative control, and a separate
production-bundle smoke. G47’s same-call Health sample correction is in
`600aadd3`, with 20 full parallel `copy_owner_health` runs and a default-feature
compiler control. The Lead retains acceptance authority; see the G4 handback for
exact apparatus source, counts, evidence and artifact hashes.

Run permanent `central_staging_test.py`, private namespace guard tests, language
checks, and each group on the real Worker and both Router binaries. Keep failing
controls at the deciding boundaries; restore sources for the final run. Build
production and conformance separately with the public Worker README commands
and bundle guards. Retain both hashes and the workerd/Miniflare tool root.
Local process reopen does not prove hosted deployment or platform eviction.

No existing object-construction/startup signal has been identified. Report
“eviction not evidenced; state and session survival across the idle wait
observed” unless an existing platform signal establishes construction. Request
invocation IDs, cf-ray and elapsed time cannot establish a new object instance.
No product instrumentation is authorized by this apparatus commission.

Declared address completeness is an input to compare, not a substitute for
collecting and inspecting live routing/settings/version responses. Staging
stays unknown until genuine Access, all alternate addresses, header transit,
actual guarded deploy and owner/read-copy outcomes are observed on the pinned
target. Retain every gap; the driver does not issue delivery acceptance.

## Cleanup

Stop only recorded children and exact fixture sockets; reap workerd before
reopening its persistence and drain both isolated Hosts. Retain evidence and
state for independent readback, then remove only owned task paths when their
consumers release them. Never delete staging scripts, namespaces or credentials
as journey cleanup. Routing rollback belongs to the private operator procedure.
