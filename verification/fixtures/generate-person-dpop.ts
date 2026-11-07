// Synthetic test keys only, derived from fixed public fixture seeds. Never use for access.
import { createECDH, createPrivateKey, createPublicKey, createHash } from "node:crypto";
import { writeFile } from "node:fs/promises";
import { SignJWT, calculateJwkThumbprint, importJWK, type JWK, type JWTPayload } from "jose";

const fixtureUrl = new URL(
  "../../crates/collaboration-central-core/tests/fixtures/person-dpop.json",
  import.meta.url,
);
const nowMs = 1_793_923_200_000;
const nowSeconds = nowMs / 1000;
const personId = "01a1b000-0000-7000-8000-000000000001";
const sessionId = "01a1b000-0000-7000-8000-000000000002";
const organizationId = "01a1b000-0000-7000-8000-000000000003";
const operationId = "01a1b000-0000-7000-8000-000000000004";
const workspaceId = "01a1b000-0000-7000-8000-000000000005";
const policy = {
  issuer: "https://signin.example.test",
  audience: "https://fleet.example.test",
  public_htu: "https://fleet.example.test/admin/workspaces",
};
const operation = {
  kind: "renameWorkspace",
  organization_id: organizationId,
  operation_id: operationId,
  workspace_id: workspaceId,
  display_name: 'Research "α"\nworkspace',
};
const intent = hash(
  JSON.stringify([
    "fleet-person-intent-prototype-v1",
    organizationId,
    personId,
    operationId,
    "renameWorkspace",
    workspaceId,
    operation.display_name,
  ]),
);

function hash(value: string): string {
  return createHash("sha256").update(value).digest("base64url");
}
function requestId(atMs: number): string {
  const time = atMs.toString(16).padStart(12, "0");
  return `${time.slice(0, 8)}-${time.slice(8)}-7000-8000-000000000006`;
}
function ed25519Jwk(seedByte: number): JWK {
  const privateKey = createPrivateKey({
    key: Buffer.concat([
      Buffer.from("302e020100300506032b657004220420", "hex"),
      Buffer.alloc(32, seedByte),
    ]),
    format: "der",
    type: "pkcs8",
  });
  return {
    ...privateKey.export({ format: "jwk" }),
    ...createPublicKey(privateKey).export({ format: "jwk" }),
    alg: "EdDSA",
    kid: "prototype-issuer",
    use: "sig",
  };
}
function es256Jwk(seedByte: number): JWK {
  const curve = createECDH("prime256v1");
  curve.setPrivateKey(Buffer.alloc(32, seedByte));
  const point = curve.getPublicKey(undefined, "uncompressed");
  return {
    kty: "EC",
    crv: "P-256",
    x: point.subarray(1, 33).toString("base64url"),
    y: point.subarray(33).toString("base64url"),
    d: curve.getPrivateKey().toString("base64url"),
  };
}
function publicJwk(privateJwk: JWK): JWK {
  const { d: _private, ...publicKey } = privateJwk;
  return publicKey;
}
const issuerJwk = ed25519Jwk(1);
const proofJwk = es256Jwk(7);
const otherProofJwk = es256Jwk(9);
const issuerKey = await importJWK(issuerJwk, "EdDSA");
const proofKey = await importJWK(proofJwk, "ES256");
const otherIssuerKey = await importJWK(ed25519Jwk(2), "EdDSA");
const otherProofKey = await importJWK(otherProofJwk, "ES256");
const thumbprint = await calculateJwkThumbprint(publicJwk(proofJwk));
const trustedJwks = JSON.stringify({ keys: [publicJwk(issuerJwk)] });
const tokenClaims = {
  iss: policy.issuer,
  aud: policy.audience,
  sub: personId,
  sid: sessionId,
  organization_id: organizationId,
  token_profile: "person-fleet",
  iat: nowSeconds - 10,
  exp: nowSeconds + 300,
  cnf: { jkt: thumbprint },
  session_expires_at_ms: nowMs + 600_000,
  authorization_snapshot_at_ms: nowMs - 10_000,
  authorization_snapshot_valid_until_ms: nowMs + 290_000,
};
async function signToken(claims: JWTPayload, key = issuerKey): Promise<string> {
  return new SignJWT(claims)
    .setProtectedHeader({ alg: "EdDSA", kid: "prototype-issuer", typ: "JWT" })
    .sign(key);
}
const token = await signToken(tokenClaims);
const proofClaims = {
  jti: requestId(nowMs),
  htm: "POST",
  htu: policy.public_htu,
  iat: nowSeconds,
  ath: hash(token),
  intent,
};
async function signProof(
  claims: JWTPayload,
  key = proofKey,
  jwk = publicJwk(proofJwk),
): Promise<string> {
  return new SignJWT(claims).setProtectedHeader({ typ: "dpop+jwt", alg: "ES256", jwk }).sign(key);
}
const proof = await signProof(proofClaims);
const input = {
  authorization: `DPoP ${token}`,
  proof,
  request_id: requestId(nowMs),
  method: "POST",
  body: JSON.stringify(operation),
  reject_through_ms: nowMs - 65_001,
};
interface Vector {
  name: string;
  input: typeof input;
  now_ms: number;
  expected: string;
}
const vectors: Vector[] = [];
function add(
  name: string,
  changes: Partial<typeof input> = {},
  expected = "verified",
  atMs = nowMs,
): void {
  vectors.push({ name, input: { ...input, ...changes }, now_ms: atMs, expected });
}
async function proofCase(name: string, changes: JWTPayload, expected: string): Promise<void> {
  add(name, { proof: await signProof({ ...proofClaims, ...changes }) }, expected);
}
async function tokenCase(name: string, changes: JWTPayload, expected: string): Promise<void> {
  const changedToken = await signToken({ ...tokenClaims, ...changes });
  add(
    name,
    {
      authorization: `DPoP ${changedToken}`,
      proof: await signProof({ ...proofClaims, ath: hash(changedToken) }),
    },
    expected,
  );
}

add("valid");
add("body-key-order-and-whitespace-are-not-intent", {
  body: JSON.stringify(
    {
      display_name: operation.display_name,
      workspace_id: workspaceId,
      operation_id: operationId,
      organization_id: organizationId,
      kind: "renameWorkspace",
    },
    null,
    2,
  ),
});
add("bearer-downgrade", { authorization: `Bearer ${token}` }, "dpopRequired");
add(
  "changed-body",
  { body: JSON.stringify({ ...operation, display_name: "stolen intent" }) },
  "intent",
);
add(
  "changed-operation-id",
  { body: JSON.stringify({ ...operation, operation_id: workspaceId }) },
  "intent",
);
add(
  "changed-target-workspace",
  { body: JSON.stringify({ ...operation, workspace_id: operationId }) },
  "intent",
);
add(
  "extra-unsigned-body-field",
  { body: JSON.stringify({ ...operation, privileged: true }) },
  "operationBody",
);
const wrongToken = await signToken(tokenClaims, otherIssuerKey);
add("issuer-wrong-signing-key", { authorization: `DPoP ${wrongToken}` }, "tokenSignature");
add(
  "proof-wrong-signing-key",
  { proof: await signProof(proofClaims, otherProofKey) },
  "proofSignature",
);
add(
  "proof-other-key-valid-signature",
  { proof: await signProof(proofClaims, otherProofKey, publicJwk(otherProofJwk)) },
  "keyBinding",
);
await proofCase("changed-signed-intent", { intent: hash("other operation") }, "intent");
await proofCase("wrong-htu", { htu: "https://attacker.example.test/admin/workspaces" }, "target");
await proofCase("wrong-method", { htm: "GET" }, "method");
await proofCase("old-iat-at-exclusive-boundary", { iat: nowSeconds - 65 }, "proofTime");
await proofCase("old-iat-within-window", { iat: nowSeconds - 64 }, "verified");
await proofCase("future-iat-at-inclusive-boundary", { iat: nowSeconds + 5 }, "verified");
await proofCase("future-iat-outside-window", { iat: nowSeconds + 6 }, "proofTime");
const { ath: _missingAth, ...withoutAth } = proofClaims;
add("missing-ath", { proof: await signProof(withoutAth) }, "tokenHash");
await proofCase("wrong-ath", { ath: hash("different token") }, "tokenHash");
await proofCase("jti-not-request-id", { jti: workspaceId }, "requestId");
for (const [name, timestampMs, expected] of [
  ["request-at-age-boundary", nowMs - 65_000, "requestTime"],
  ["request-above-age-boundary", nowMs - 64_999, "verified"],
  ["request-at-future-boundary", nowMs + 5_000, "verified"],
  ["request-after-future-boundary", nowMs + 5_001, "requestTime"],
] as const) {
  const id = requestId(timestampMs);
  add(name, { request_id: id, proof: await signProof({ ...proofClaims, jti: id }) }, expected);
}
add("request-at-watermark", { reject_through_ms: nowMs }, "requestTime");
const uuidV4 = "00000000-0000-4000-8000-000000000006";
add(
  "request-not-uuidv7",
  { request_id: uuidV4, proof: await signProof({ ...proofClaims, jti: uuidV4 }) },
  "requestId",
);
await tokenCase("wrong-issuer", { iss: "https://other.example.test" }, "issuer");
await tokenCase("wrong-audience", { aud: "other-resource" }, "audience");
await tokenCase("audience-array", { aud: ["other-resource", policy.audience] }, "verified");
await tokenCase("wrong-token-profile", { token_profile: "machine-fleet" }, "profile");
await tokenCase("native-exp-equality-others-live", { exp: nowSeconds }, "expired");
await tokenCase(
  "session-expiry-equality-jwt-snapshot-live",
  { session_expires_at_ms: nowMs },
  "expired",
);
await tokenCase(
  "snapshot-expiry-equality-jwt-session-live",
  { authorization_snapshot_valid_until_ms: nowMs },
  "expired",
);
await tokenCase(
  "snapshot-lifetime-over-300s",
  { authorization_snapshot_valid_until_ms: nowMs + 290_001 },
  "expiryBounds",
);
await tokenCase(
  "snapshot-redated-into-future",
  { authorization_snapshot_at_ms: nowMs + 1 },
  "expiryBounds",
);
await tokenCase(
  "session-not-live-at-snapshot",
  { session_expires_at_ms: nowMs - 10_000 },
  "expiryBounds",
);
await tokenCase(
  "unsafe-signed-millisecond-bound",
  { session_expires_at_ms: Number.MAX_SAFE_INTEGER + 1 },
  "expiryBounds",
);
await tokenCase(
  "missing-snapshot-bound",
  { authorization_snapshot_valid_until_ms: undefined },
  "personClaims",
);
await tokenCase("expired-native-with-positive-exp", { exp: nowSeconds - 1 }, "expired");
add("clock-advanced-after-ingress", {}, "expired", nowMs + 290_000);

// Headers are separately signed, so header-policy tests reach that rule honestly.
add(
  "wrong-proof-type",
  {
    proof: await new SignJWT(proofClaims)
      .setProtectedHeader({ typ: "JWT", alg: "ES256", jwk: publicJwk(proofJwk) })
      .sign(proofKey),
  },
  "unsupportedHeader",
);
add(
  "private-proof-jwk",
  { proof: await signProof(proofClaims, proofKey, proofJwk) },
  "unsupportedHeader",
);
add(
  "unknown-critical-header",
  {
    proof: await new SignJWT(proofClaims)
      .setProtectedHeader({ typ: "dpop+jwt", alg: "ES256", jwk: publicJwk(proofJwk), extra: true })
      .sign(proofKey),
  },
  "unsupportedHeader",
);

await writeFile(
  fixtureUrl,
  `${JSON.stringify({ generator: "JOSE 6.2.12; synthetic fixed seeds; ECDSA signatures generated once and checked in", policy, trusted_jwks: trustedJwks, now_ms: nowMs, thumbprint, intent, vectors }, null, 2)}\n`,
);
console.log(`wrote ${vectors.length} JOSE vectors to ${fixtureUrl.pathname}`);
