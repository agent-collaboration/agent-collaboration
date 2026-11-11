// The message-board text and identity types main decodes carried Fleet and
// enrollment columns into, ported exactly (crates/message-board at this
// checkout). Main reads each through `canonical` (fleet_column_codec.rs:50–60)
// or an equivalent round trip (machine_enrollment_record.rs:44–46,
// fleet_record_codec.rs:52–60): a stored value that its type would normalize is
// refused, not rewritten. Each function returns null when main accepts the
// value exactly as stored, or the reason it refuses it.

const utf8Length = (value: string): number => new TextEncoder().encode(value).length;

// Rust str::trim: Unicode White_Space (board_metadata.rs:12–13), not JavaScript's \s.
const RUST_WHITE_SPACE =
  "\\u0009-\\u000d\\u0020\\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000";
const RUST_TRIM = new RegExp(`^[${RUST_WHITE_SPACE}]+|[${RUST_WHITE_SPACE}]+$`, "gu");
export const rustTrim = (value: string): string => value.replace(RUST_TRIM, "");
// C0 controls other than LF and TAB (board_metadata.rs:50–52).
const hasForbiddenC0 = (value: string): boolean =>
  [...value].some((character) => {
    const code = character.codePointAt(0) ?? 0;
    return code <= 0x1f && code !== 0x0a && code !== 0x09;
  });

/** board_metadata.rs `bounded_text!`: optional trim, a UTF-8 byte bound, optional C0 refusal. */
function boundedText(min: number, max: number, trim: boolean, rejectC0: boolean) {
  return (value: string): string | null => {
    const canonical = trim ? rustTrim(value) : value;
    if (canonical !== value) return "has surrounding whitespace its type would trim";
    const bytes = utf8Length(canonical);
    if (bytes < min || bytes > max) return `must be ${min} to ${max} UTF-8 bytes`;
    if (rejectC0 && hasForbiddenC0(canonical)) return "contains a forbidden control character";
    return null;
  };
}

export const resourceName = boundedText(1, 256, true, false);
export const description = boundedText(0, 16 * 1024, false, false);
export const markdownText = boundedText(1, 64 * 1024, false, true);
export const machineLabel = boundedText(1, 256, true, true);

/** board_identity.rs `validated_identity_string!` with its byte bound and validator. */
function identity(
  min: number,
  max: number,
  valid: (value: string) => boolean,
  requirement: string,
) {
  return (value: string): string | null => {
    const bytes = utf8Length(value);
    return bytes >= min && bytes <= max && valid(value) ? null : requirement;
  };
}
/**
 * ServiceId (board_identity.rs:154–164): any UUID that parses and is already in
 * canonical lowercase hyphenated spelling, of any version and variant.
 */
export const serviceId = identity(
  36,
  36,
  (value) => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value),
  "must be a canonical lowercase UUID",
);
export const endpointId = identity(
  1,
  64,
  (value) => /^[a-z][a-z0-9-]*$/.test(value),
  "must start with a lowercase letter and contain only lowercase letters, digits, or hyphens",
);
export const sessionId = identity(
  1,
  4096,
  (value) => !value.includes("\0"),
  "must contain 1 to 4096 UTF-8 bytes without NUL",
);
export const humanId = sessionId;

/** repository_identity.rs normalize_git_origin_url (:117–149). */
export function normalizeGitOrigin(origin: string): string | null {
  const trimmed = (rustTrim(origin).split(/[?#]/)[0] ?? "").replace(/\/+$/, "");
  if (trimmed === "") return null;
  let host: string;
  let path: string;
  const scheme = trimmed.indexOf("://");
  if (scheme >= 0) {
    const remainder = trimmed.slice(scheme + 3);
    const at = remainder.lastIndexOf("@");
    const afterUser = at >= 0 ? remainder.slice(at + 1) : remainder;
    const slash = afterUser.indexOf("/");
    if (slash < 0) return null;
    [host, path] = [afterUser.slice(0, slash), afterUser.slice(slash + 1)];
  } else if (trimmed.includes(":")) {
    const colon = trimmed.indexOf(":");
    const hostWithUser = trimmed.slice(0, colon);
    const at = hostWithUser.lastIndexOf("@");
    [host, path] = [at >= 0 ? hostWithUser.slice(at + 1) : hostWithUser, trimmed.slice(colon + 1)];
  } else {
    const slash = trimmed.indexOf("/");
    if (slash < 0) return null;
    [host, path] = [trimmed.slice(0, slash), trimmed.slice(slash + 1)];
    if (!host.includes(".")) return null;
  }
  host = rustTrim(host).toLowerCase();
  path = path.replace(/^\/+|\/+$/g, "");
  if (path.endsWith(".git")) path = path.slice(0, -4);
  return host !== "" && path !== "" ? `${host}/${path}` : null;
}
/** NormalizedOrigin (repository_identity.rs:19–40): at most 4096 bytes and already canonical. */
export const normalizedOrigin = (value: string): string | null =>
  utf8Length(value) > 4096
    ? "must contain at most 4096 UTF-8 bytes"
    : normalizeGitOrigin(value) === value
      ? null
      : "must already be a canonical repository origin";
/** CommonDirectory (repository_identity.rs:66–81): an absolute path without NUL, at most 4096 bytes. */
export const commonDirectory = (value: string): string | null =>
  utf8Length(value) > 4096 || value.includes("\0") || !value.startsWith("/")
    ? "must be an absolute path without NUL, at most 4096 UTF-8 bytes"
    : null;
/** CommitSha (record_references.rs `digest_value!`): 40 or 64 lowercase hexadecimal characters. */
export const commitSha = (value: string): string | null =>
  /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(value)
    ? null
    : "must be a complete lowercase hexadecimal digest";

/**
 * A Router time as main stores it (fleet_record_codec.rs:39–47): chrono's
 * `to_rfc3339` (+00:00) or the `Z` form, with fractional seconds only as
 * chrono's AutoSi prints them (none, or exactly 3, 6 or 9 digits, shortest).
 */
export function canonicalRouterTime(value: string): string | null {
  const match =
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{3}|\d{6}|\d{9}))?(?:\+00:00|Z)$/.exec(
      value,
    );
  if (match === null) return "is not a canonical RFC 3339 UTC time";
  const [, year, month, day, hour, minute, second, fraction] = match;
  const daysInMonth = new Date(Date.UTC(Number(year), Number(month), 0)).getUTCDate();
  if (Number(month) < 1 || Number(month) > 12 || Number(day) < 1 || Number(day) > daysInMonth)
    return "is not a calendar date";
  if (Number(hour) > 23 || Number(minute) > 59 || Number(second) > 59)
    return "is not a time of day";
  if (
    fraction !== undefined &&
    (/^0+$/.test(fraction) || (fraction.length > 3 && fraction.endsWith("000")))
  )
    return "has fractional digits chrono would not print";
  return null;
}
