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

Host lifecycle orchestration, the full process-crash matrix, richer frozen
mail/cancel state, remote hardware and staging corpus remain unverified.
