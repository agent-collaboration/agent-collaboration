# Staging cutover tests

Unit tests are colocated with the Rust implementation in `src/*_test.rs`.
They cover transport reframing, unchanged row bytes, source cursor separation,
the existing Host singleton lock, private roots and retained source identities.

Run them through the repository signing runner with Cargo nextest:

```sh
cargo nextest run -p board-staging-cutover --profile ci
```

The `tests/integration/main.rs` aggregate exercises a real TestCentral import,
machine-session Claim and SDK staging/verification/publication of an e2 writer
whose unchanged snapshot still has its e1 seed position. It checks offline
preflight, paging and exact installed replicated-table hashes before any new
application write. The retained-origin mismatch unit case remains separate.

Owner publication tests use the existing private Owner credential format and
actual authenticated HTTP routes. Successful response suppression at Begin,
Page and Complete exercises reconciliation of the same sealed run and import
identity. The HTTP edge forwards real requests before withholding one answer;
these cases do not stand in for the full process-crash matrix.

Archive cases preserve the original directory inode and all native/sibling
files, reconcile a discarded local result, and refuse changed sources,
colliding destinations, another run's confirmation and modified backups.
Sealing and archiving share the tested OS no-overwrite move primitive.

The `tests/e2e/main.rs` aggregate launches the compiled helper through the
signing runner. Inspection preserves a running Host's authority and refuses
another declared service. The offline command publishes through real owner
HTTP routes, archives the original directory and reconciles the same bundle
and import on rerun. A held committed Fleet answer checks that the helper
retains the source Host lock throughout publication. Changing a frozen claim
request refuses the rerun without changing the bundle or backup.

The installed-content process case starts an actual isolated Host with the
shared native-protocol and router-health fixtures. Its public Claim installs
e2. After orderly Host and native-child exit, cold inspection compares the
actual e2 replicated tables with the prepared snapshot and rejects an altered
expected digest for a source containing the initial project record. Two
ignored child entry points are apparatus invoked by this parent test.

For an emitted notification intent, the SDK component case proves exact table
hashes before application activation. The actual Host case checks normal
recovery separately: one handoff, one request decision, two appended feed
entries, the inherited epoch boundary and the subscription-delivery actor.
Prior rows remain exact and every other table digest is unchanged. An extra
recovery decision fails the proof. A second Host lifetime replays Claim,
appends and replays a public trail canary, and reads one intact canary record
at the head after recovery plus one. Cold counts reject duplicate effects.

The native-epoch cases use a real owner takeover to a second enrolled Host.
An actual e3 file with unchanged application epoch-one history must retain every
table hash and remain an admissible source. A public write on that holder is
read back through the original application cursor; the resulting application
epoch-three source must refuse before writable admission, even while another
retained reader holds the file. Takeover assigns its recipient directly, so
these cases await its public writer view rather than issuing another Claim.

The canary readback case enrolls another actual isolated Host with no local
project source. It checks that Host's distinct service identity, reads the same
frozen canary body/request/cursor through the public protocol, and requires a
Central read copy while e2 remains the writer. Selecting the live producer
instead fails the identity check. Both Host/native lifecycles are joined. This
is a local two-Host proof, with separate enrollments on one physical machine.

The compiled `claim` command reopens the same sealed plan and validates the
running Host's enrollment origin, preserved source backup and Central's exact
completed import. It submits and replays the frozen ordinary Claim, then the
process test checks the actual cold e2 tables. Its report covers the Claim phase
only. A false success without that RPC fails the missing-install assertion.
Separate tests reject an absent backup, a local bundle without completed
publication, and a wrong origin before consuming Owner authentication.

Claim CLI proof uses the existing SelfHosted assertion transport directly to
TestCentral as a local stand-in. Cloudflare Access transport for the Claim phase
remains unverified until declared staging execution exercises it end to end.

The richer L2 process case uses separate old/new Central lifetimes. Ordinary
public mutations populate work, dependency links, subtask history, roles,
facts, plan revisions, threads/refilings/posts/references, participation, covered
read marks, legacy watches and explicit Poll membership. It carries the original
Fleet graph forward verbatim and compares the installed writer with an
independent pre-export SDK manifest. Line-two inheritance, cursor and record time
remain exact. Public readback preserves a fact integer above 2^53 and escaped
Unicode text; replaying the old task request adds no history. A seeded loss of
task-link rows prevents publication.

The offline `verify` command holds the existing Host lock, checks the same sealed
run and original backup, and compares the actual installed e2 tables plus
application history metadata. Exact equality returns `contentVerified`.
Changed content returns `contentUnverified` with both manifests and nonzero
exit. Tests reject selecting the old source epoch and falsely accepting an
unrelated public write. Named legitimate startup recovery is not yet accepted
by this operator phase; it remains changed/unverified until semantic support
lands. This command does not declare the whole cutover complete.

Host lifecycle orchestration, the full process-crash matrix, frozen
mail/cancel state, remote hardware and staging corpus remain unverified.
