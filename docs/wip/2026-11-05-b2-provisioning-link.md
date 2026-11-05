# B2 provisioning link contract

Consumer: Switchboard sign-in hosts. This contract adopts the unchanged F1c ProvisionOrganization payload/result on the D1 base. It does not establish that the route or SQL transaction is implemented; real link and provisioning proof follow the owner SQL approval.

## Request

POST /internal/organization-provisioning with Content-Type: application/json. The body is exactly {"organizationId":"<canonical lowercase UUIDv7>"}; additional fields and retired people/standing operations are refused. The Rust authority is identity_contract/organization_provisioning.rs; JSON Schema and fixtures are under crates/collaboration-central-core/contracts/ProvisionOrganization.*. These artifacts are adopted byte-for-byte from F1c86bf5f19e75dc9eb1a4151a179d21de7fda457a2.

Cloudflare sends this request through the sign-in Worker’s CENTRAL binding. Native Node sends it through the deployment-owned Unix socket to Tokio. The link proves which service called: this operation adds no person token, callback, service credential or signed envelope. The public sign-in edge exposes no route that forwards an internal request from an internet caller.

## Reply and retry

Only HTTP200 with exactly {"result":"provisioned"} confirms provisioning. The closed Rust result has no refused/admitted/standing variant. Non200, connection failure, lost reply or malformed200 is never a confirmed provisioning result: the Switchboard adapter returns its existing unknown result and the organization remains settingUp. The adapter may classify transport failure as unavailable, timeout or malformed-reply using its existing closed API. It must not invent a central success from an error body. Error-body details are not a consumer contract.

ProvisionOrganization uses organizationId itself as its idempotency identity. It has no new operationId. One invocation makes one plain call. Nothing automatically resends it; the sign-in owner page’s explicit Try again action makes the same call while settingUp. Every successful repeat returns provisioned without another stamp or setup record. This follows the newer B2 doc and Lead01a1aba1-d3cd retry ruling.

## Proof boundary

The two permanent aggregate integration cases compare Rust serialization and generated JSON Schema with F1c literals, including retired request/result negatives. They do not prove link reachability, stamp/mismatch policy, atomic record write, live signing, or the real settingUp→ready journey. The table SQL and the B2c confirmation envelope/replay-state home remain separately gated. No automatic retry, admission workflow or table-backed stand-in is introduced here.

## Component checkpoint

The core now supplies organization_provisioning_router and an OrganizationProvisioningPort capability. HTTP400 carries the fixed wrong-operation refusal for an invalid body; a mismatched organization is HTTP403 with an empty body; unavailable or unconfirmed commit is HTTP503 with an empty body. The consumer still treats every non200 as unknown and parses no error body as a ProvisionOrganizationResult. Method/path selection precedes body decoding. One admitted HTTP attempt invokes the capability exactly once.

The concrete Fleet transaction capability and both hosts’ mount remain unimplemented while owner SQL and project-affinity decisions are pending. The HTTP tests use a RecordingProvisioner outside the parsing/status/attempt-count claims; it records no database state. This allows transport policy to progress, at the cost of unverified storage and physical-link integration. Closure requires the actual Fleet owner and real Cloudflare/Unix mounts passing the joint journey. The pure OrganizationIdentity rules separately pin unstamped, same-organization and different-organization decisions; they do not enforce concurrency or persistence.

## Retired central people commands

CreateOrganization, SetOrganizationOwner, AllowSignIn and RemoveSignIn are removed from central’s Rust surface and published schema artifacts. Better Auth owns those changes. The original positive request bodies remain verbatim in tests/fixtures/retired_people_commands.json as negative provisioning inputs; a nonempty aggregate case checks both the real Rust decoder and its generated schema refuse every one. This is the requested contract retirement, not a failing-test exemption. Generic Admitted and CommandStatus contracts remain untouched for B5’s later decision.
