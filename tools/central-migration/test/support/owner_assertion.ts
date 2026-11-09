// Test only: a signing key the test serves as the team's JWKS, and Access
// assertions signed with it. Times are explicit arguments, so no test waits.

import { ACCESS } from "./workerd_harness.ts";

const encode = (bytes: Uint8Array): string => Buffer.from(bytes).toString("base64url");
const encodeJson = (value: unknown): string =>
  encode(new TextEncoder().encode(JSON.stringify(value)));

export interface TestSigner {
  readonly jwks: { keys: unknown[] };
  sign(claims: Record<string, unknown>): Promise<string>;
}

export async function testSigner(kid = "migration-test-key"): Promise<TestSigner> {
  const pair = await crypto.subtle.generateKey(
    {
      name: "RSASSA-PKCS1-v1_5",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey);
  return {
    jwks: { keys: [{ kid, kty: "RSA", n: publicJwk.n, e: publicJwk.e, alg: "RS256", use: "sig" }] },
    async sign(claims) {
      const signed = `${encodeJson({ alg: "RS256", kid })}.${encodeJson(claims)}`;
      const signature = await crypto.subtle.sign(
        "RSASSA-PKCS1-v1_5",
        pair.privateKey,
        new TextEncoder().encode(signed),
      );
      return `${signed}.${encode(new Uint8Array(signature))}`;
    },
  };
}

/** The claims Access issues for an identity (owner) caller, valid around `nowSeconds`. */
export function ownerClaims(nowSeconds: number, email: string = ACCESS.ACCESS_OWNER_IDENTITY) {
  return {
    iss: `https://${ACCESS.ACCESS_TEAM_DOMAIN}`,
    aud: [ACCESS.ACCESS_ADMIN_AUD],
    exp: nowSeconds + 3600,
    nbf: nowSeconds - 60,
    type: "app",
    email,
    sub: "owner-subject",
  };
}
