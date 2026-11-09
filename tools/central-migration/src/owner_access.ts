// Owner authentication for the migration's admin routes.
//
// This is central's own owner check, mirrored rule for rule so the migration
// adds no new auth design. Central's checks live in main (b2a0d2280),
// crates/collaboration-central-core/src/:
// - policy: access_contract.rs:82–138;
// - assertion parse: access_rules.rs:52–125;
// - JWKS key selection: access_contract.rs:156–168;
// - key profile: access_key_profile.rs:6–37;
// - route authorization: access_rules.rs:143–162.
// Only the owner caller class is admitted: these routes are /admin/v1/.

export type AccessRefusal =
  | "missingAssertion"
  | "malformedAssertion"
  | "unsupportedAlgorithm"
  | "wrongIssuer"
  | "wrongAudience"
  | "expired"
  | "notYetValid"
  | "invalidCaller"
  | "unknownKey"
  | "invalidKey"
  | "badSignature"
  | "keySourceUnavailable"
  | "wrongCallerClass"
  | "wrongOwner"
  | "routeRefused";

export interface AccessPolicy {
  readonly issuer: string;
  readonly certsUrl: string;
  readonly sessionAudience: string;
  readonly adminAudience: string;
  readonly ownerIdentity: string;
}

export interface AccessSettings {
  readonly ACCESS_TEAM_DOMAIN: string;
  readonly ACCESS_SESSION_AUD: string;
  readonly ACCESS_ADMIN_AUD: string;
  readonly ACCESS_OWNER_IDENTITY: string;
}

export type OwnerCheck =
  | { readonly kind: "owner"; readonly email: string }
  | { readonly kind: "refused"; readonly refusal: AccessRefusal };

interface AccessJwk {
  readonly kid: string;
  readonly kty: string;
  readonly n: string;
  readonly e: string;
  readonly alg?: string | undefined;
  readonly use?: string | undefined;
}

/** Fetches the team's JWKS document; any failure is "key source unavailable". */
export type FetchAccessKeys = (certsUrl: string) => Promise<unknown>;

class Refused extends Error {
  readonly refusal: AccessRefusal;
  constructor(refusal: AccessRefusal) {
    super(refusal);
    this.refusal = refusal;
  }
}

const refuse = (refusal: AccessRefusal): never => {
  throw new Refused(refusal);
};

/** access_contract.rs:82–117: the team domain and audiences central accepts. */
export function accessPolicy(settings: AccessSettings): AccessPolicy {
  const team = settings.ACCESS_TEAM_DOMAIN;
  if (!team.endsWith(".cloudflareaccess.com") || /[^A-Za-z0-9.-]/.test(team)) {
    refuse("invalidCaller");
  }
  const issuer = `https://${team}`;
  const policy: AccessPolicy = {
    issuer,
    certsUrl: `${issuer}/cdn-cgi/access/certs`,
    sessionAudience: settings.ACCESS_SESSION_AUD,
    adminAudience: settings.ACCESS_ADMIN_AUD,
    ownerIdentity: settings.ACCESS_OWNER_IDENTITY,
  };
  if (
    !validKeyUrl(policy.issuer) ||
    !validKeyUrl(policy.certsUrl) ||
    policy.sessionAudience === "" ||
    policy.adminAudience === "" ||
    policy.sessionAudience === policy.adminAudience ||
    policy.ownerIdentity.trim() === ""
  ) {
    refuse("invalidCaller");
  }
  return policy;
}

/** access_contract.rs:128–139. */
function validKeyUrl(value: string): boolean {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return false;
  }
  return (
    (url.protocol === "http:" || url.protocol === "https:") &&
    url.hostname !== "" &&
    url.username === "" &&
    url.password === "" &&
    url.search === "" &&
    url.hash === "" &&
    value.trim() === value &&
    !/\p{Cc}/u.test(value)
  );
}

function decodeSegment(segment: string): Uint8Array<ArrayBuffer> {
  if (!/^[A-Za-z0-9_-]*$/.test(segment) || segment.length % 4 === 1) {
    return refuse("malformedAssertion");
  }
  const binary = atob(segment.replaceAll("-", "+").replaceAll("_", "/"));
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

function decodeJson(segment: string): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(decodeSegment(segment)));
  } catch (error) {
    if (error instanceof Refused) throw error;
    return refuse("malformedAssertion");
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return refuse("malformedAssertion");
  }
  return value as Record<string, unknown>;
}

const text = (value: unknown): string =>
  typeof value === "string" ? value : refuse("malformedAssertion");
const integer = (value: unknown): number =>
  typeof value === "number" && Number.isSafeInteger(value) ? value : refuse("malformedAssertion");
const optionalText = (value: unknown): string | undefined =>
  value === undefined || value === null ? undefined : text(value);

interface ParsedAssertion {
  readonly kid: string;
  readonly signed: Uint8Array<ArrayBuffer>;
  readonly signature: Uint8Array<ArrayBuffer>;
  readonly caller:
    | { readonly kind: "machine"; readonly clientId: string }
    | { readonly kind: "owner"; readonly email: string };
}

/** access_rules.rs:52–125, for an /admin/v1/ path (the admin audience). */
function parseAdminAssertion(
  token: string | null,
  policy: AccessPolicy,
  nowSeconds: number,
): ParsedAssertion {
  if (token === null) return refuse("missingAssertion");
  const parts = token.split(".");
  if (parts.length !== 3) return refuse("malformedAssertion");
  const [head = "", body = "", tail = ""] = parts;
  const header = decodeJson(head);
  const alg = text(header.alg);
  const kid = text(header.kid);
  if (alg !== "RS256") refuse("unsupportedAlgorithm");
  if (kid === "") refuse("unknownKey");
  const claims = decodeJson(body);
  const iss = text(claims.iss);
  const aud = claims.aud;
  const audiences = typeof aud === "string" ? [aud] : Array.isArray(aud) ? aud.map(text) : null;
  if (audiences === null) return refuse("malformedAssertion");
  const exp = integer(claims.exp);
  const nbf = claims.nbf === undefined || claims.nbf === null ? undefined : integer(claims.nbf);
  const type = text(claims.type);
  const commonName = optionalText(claims.common_name);
  const email = optionalText(claims.email);
  const sub = text(claims.sub);
  if (iss !== policy.issuer) refuse("wrongIssuer");
  if (!audiences.includes(policy.adminAudience)) refuse("wrongAudience");
  if (exp <= nowSeconds) refuse("expired");
  if (nbf !== undefined && nbf > nowSeconds) refuse("notYetValid");
  if (type !== "app") refuse("invalidCaller");
  let caller: ParsedAssertion["caller"];
  if (commonName !== undefined && sub === "" && (email === undefined || email === "")) {
    if (commonName === "") refuse("invalidCaller");
    caller = { kind: "machine", clientId: commonName };
  } else if (commonName === undefined && email !== undefined && email.trim() !== "" && sub !== "") {
    caller = { kind: "owner", email };
  } else {
    return refuse("invalidCaller");
  }
  return {
    kid,
    signed: new TextEncoder().encode(`${head}.${body}`),
    signature: decodeSegment(tail),
    caller,
  };
}

/** access_contract.rs:156–168 and access_key_profile.rs:6–37. */
function selectKey(document: unknown, kid: string): AccessJwk {
  if (typeof document !== "object" || document === null) return refuse("keySourceUnavailable");
  const keys = (document as { keys?: unknown }).keys;
  if (!Array.isArray(keys)) return refuse("keySourceUnavailable");
  const matching = keys.filter(
    (key): key is AccessJwk =>
      typeof key === "object" && key !== null && (key as { kid?: unknown }).kid === kid,
  );
  const [key, second] = matching;
  if (key === undefined) return refuse("unknownKey");
  if (
    second !== undefined ||
    key.kty !== "RSA" ||
    typeof key.n !== "string" ||
    typeof key.e !== "string" ||
    key.n === "" ||
    key.e === "" ||
    (key.alg !== undefined && key.alg !== "RS256") ||
    (key.use !== undefined && key.use !== "sig")
  ) {
    return refuse("invalidKey");
  }
  const modulus = unsignedComponent(decodeKeyPart(key.n));
  const exponent = unsignedComponent(decodeKeyPart(key.e));
  const first = modulus[0] ?? refuse("invalidKey");
  const bits = modulus.length * 8 - (Math.clz32(first) - 24);
  const last = modulus[modulus.length - 1] ?? refuse("invalidKey");
  if (bits < 2048 || bits > 4096 || last % 2 === 0) refuse("invalidKey");
  if (exponent.length > 5) refuse("invalidKey");
  const exponentValue = exponent.reduce((value, byte) => value * 256 + byte, 0);
  if (exponentValue < 3 || exponentValue > 2 ** 33 - 1 || exponentValue % 2 === 0) {
    refuse("invalidKey");
  }
  return key;
}

function decodeKeyPart(value: string): Uint8Array {
  try {
    return decodeSegment(value);
  } catch {
    return refuse("invalidKey");
  }
}

function unsignedComponent(bytes: Uint8Array): Uint8Array {
  const start = bytes.findIndex((byte) => byte !== 0);
  return start < 0 ? refuse("invalidKey") : bytes.subarray(start);
}

async function signatureValid(key: AccessJwk, assertion: ParsedAssertion): Promise<boolean> {
  let imported: CryptoKey;
  try {
    imported = await crypto.subtle.importKey(
      "jwk",
      { kty: "RSA", n: key.n, e: key.e, alg: "RS256", ext: true },
      { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
      false,
      ["verify"],
    );
  } catch {
    return refuse("invalidKey");
  }
  try {
    return await crypto.subtle.verify(
      "RSASSA-PKCS1-v1_5",
      imported,
      assertion.signature,
      assertion.signed,
    );
  } catch {
    return refuse("badSignature");
  }
}

/**
 * The owner check for one admin request: parse, select the signing key,
 * verify the signature, then admit only the configured owner (access_rules.rs
 * 143–162). `nowSeconds` comes from the caller so tests drive time.
 */
export async function checkOwner(
  assertion: string | null,
  policy: AccessPolicy,
  nowSeconds: number,
  fetchKeys: FetchAccessKeys,
): Promise<OwnerCheck> {
  try {
    const parsed = parseAdminAssertion(assertion, policy, nowSeconds);
    let document: unknown;
    try {
      document = await fetchKeys(policy.certsUrl);
    } catch {
      return refuse("keySourceUnavailable");
    }
    const key = selectKey(document, parsed.kid);
    if (!(await signatureValid(key, parsed))) refuse("badSignature");
    if (parsed.caller.kind !== "owner") refuse("wrongCallerClass");
    const email = parsed.caller.kind === "owner" ? parsed.caller.email : "";
    if (email !== policy.ownerIdentity) refuse("wrongOwner");
    return { kind: "owner", email };
  } catch (error) {
    if (error instanceof Refused) return { kind: "refused", refusal: error.refusal };
    throw error;
  }
}
