import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { parse } from "smol-toml";
import { z } from "zod";
import { conformanceMarkers } from "./worker-bundle-markers.ts";
import { retiredMarkers } from "./worker-retired-markers.ts";

export function checkDeployTemplate(
  workerConfig: string,
  toolingConfig: string,
): string | undefined {
  for (const setting of ["workersDev: false", "previewUrls: false"])
    if (!workerConfig.includes(setting)) return `Worker deploy template must set ${setting}`;
  if (/\b(accountId|domains|routes)\s*:/.test(workerConfig))
    return "Worker public template must omit private account and routing values";
  const textValues = [...workerConfig.matchAll(/type:\s*"text",\s*value:\s*"([^"]*)"/g)].map(
    (match) => match[1],
  );
  if (
    textValues.length === 0 ||
    textValues.some((value) => value === undefined || !value.startsWith("REPLACE_"))
  )
    return "Worker public template vars must be placeholders";
  if (!toolingConfig.includes("noBundle: true"))
    return "Worker deploy tooling must upload the guarded bundle without rebundling";
  return undefined;
}

/** Each Worker entry names itself in its responses; the strings are in the wasm. */
export const publicEntryMarker = "collaboration-central-public-entry";
export const linkEntryMarker = "collaboration-central-link-entry";
export const linkPrebuild =
  "worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt --out-dir build-link -- --locked --no-default-features --features link";

/** central-link: no public surface, no Durable Object class of its own. */
export function checkLinkTemplate(linkConfig: string, toolingConfig: string): string | undefined {
  for (const setting of ["workersDev: false", "previewUrls: false"])
    if (!linkConfig.includes(setting)) return `central-link template must set ${setting}`;
  if (/\b(accountId|domains|routes)\s*:/.test(linkConfig))
    return "central-link template must omit private account and routing values";
  if (/\bexports\s*:/.test(linkConfig) || linkConfig.includes("PROJECT_COPIES"))
    return "central-link must declare no Durable Object class or Copy binding";
  if (!linkConfig.includes('entrypoint: "build-link/index.js"'))
    return "central-link template must upload the link artifact";
  if (
    !/FLEET:\s*\{\s*type:\s*"durable-object",\s*worker:\s*"replace-central-worker-name",\s*exportName:\s*"FleetObject",?\s*\}/.test(
      linkConfig,
    )
  )
    return "central-link must bind central's existing FleetObject class by central's script name";
  const textValues = [...linkConfig.matchAll(/type:\s*"text",\s*value:\s*"([^"]*)"/g)].map(
    (match) => match[1],
  );
  if (textValues.some((value) => value === undefined || !value.startsWith("REPLACE_")))
    return "central-link template vars must be placeholders";
  if (!toolingConfig.includes("noBundle: true"))
    return "Worker deploy tooling must upload the guarded bundle without rebundling";
  return undefined;
}

function booleanTuple(values: readonly boolean[]): string {
  return `(${values.map((value) => (value ? "True" : "False")).join(", ")}${values.length === 1 ? "," : ""})`;
}

export function checkWorkerBundle(
  mode: "production" | "faults" | "link",
  repositoryRoot: string = process.cwd(),
): { readonly status: number; readonly message: string } {
  if (mode === "link") return checkLinkBundle(repositoryRoot);
  const read = (path: string): string => readFileSync(join(repositoryRoot, path), "utf8");
  const manifest = z
    .object({ profile: z.record(z.string(), z.unknown()).default({}) })
    .parse(parse(read("Cargo.toml")));
  const profile = z
    .object({
      inherits: z.literal("release"),
      lto: z.literal(true),
      "opt-level": z.literal("s"),
      "codegen-units": z.literal(1),
    })
    .strict();
  if (!profile.safeParse(manifest.profile["worker-release"]).success)
    return {
      status: 1,
      message: "Worker release profile must preserve the standalone size settings",
    };
  if (
    !read("crates/collaboration-central-cloudflare/README.md").includes(
      "worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt -- --locked --no-default-features",
    )
  )
    return {
      status: 1,
      message: "Worker production prebuild must select --profile worker-release",
    };
  const artifacts = ["index.js", "index_bg.wasm"].map((name) =>
    join(repositoryRoot, "crates/collaboration-central-cloudflare/build", name),
  );
  if (artifacts.some((path) => !existsSync(path) || !statSync(path).isFile()))
    return { status: 1, message: "Worker bundle missing; build it before checking" };
  const content = Buffer.concat(
    artifacts.flatMap((path, index) =>
      index === 0 ? [readFileSync(path)] : [Buffer.from("\n"), readFileSync(path)],
    ),
  );
  if (["ADMIN_CREDENTIAL_HASH", "ACCESS_TEST_JWKS_URL"].some((marker) => content.includes(marker)))
    return {
      status: 1,
      message: "Worker bundle contains a retired credential or key-source override",
    };
  const failure = checkDeployTemplate(
    read("crates/collaboration-central-cloudflare/cloudflare.config.ts"),
    read("crates/collaboration-central-cloudflare/wrangler.config.ts"),
  );
  if (failure !== undefined) return { status: 1, message: failure };
  if (content.includes(linkEntryMarker))
    return { status: 1, message: "Worker public bundle is the central-link entry" };
  if (mode === "production" && content.includes("_cf_test_directory_updates"))
    return { status: 1, message: "Worker production bundle contains the test-only Fleet receiver" };
  if (content.includes("central depth measurement"))
    return { status: 1, message: "Worker bundle contains the temporary depth instrument" };
  if (retiredMarkers.some((marker) => content.includes(marker)))
    return { status: 1, message: "Worker bundle contains a retired central test path" };
  const present = conformanceMarkers.map((marker) => content.includes(marker));
  const expected = conformanceMarkers.map(() => mode === "faults");
  if (present.some((value, index) => value !== expected[index]))
    return {
      status: 1,
      message: `Worker ${mode} bundle marker mismatch: ${booleanTuple(present)}; expected ${booleanTuple(expected)}`,
    };
  return {
    status: 0,
    message: `Worker ${mode} bundle: conformance-control marker presence ${booleanTuple(present)}`,
  };
}

function checkLinkBundle(repositoryRoot: string): {
  readonly status: number;
  readonly message: string;
} {
  const read = (path: string): string => readFileSync(join(repositoryRoot, path), "utf8");
  if (!read("crates/collaboration-central-cloudflare/README.md").includes(linkPrebuild))
    return {
      status: 1,
      message: "central-link prebuild must select the link feature and build-link",
    };
  const artifacts = ["index.js", "index_bg.wasm"].map((name) =>
    join(repositoryRoot, "crates/collaboration-central-cloudflare/build-link", name),
  );
  if (artifacts.some((path) => !existsSync(path) || !statSync(path).isFile()))
    return { status: 1, message: "central-link bundle missing; build it before checking" };
  const content = Buffer.concat(artifacts.map((path) => readFileSync(path)));
  const failure = checkLinkTemplate(
    read("crates/collaboration-central-cloudflare/cloudflare-link.config.ts"),
    read("crates/collaboration-central-cloudflare/wrangler.config.ts"),
  );
  if (failure !== undefined) return { status: 1, message: failure };
  if (!content.includes(linkEntryMarker) || content.includes(publicEntryMarker))
    return { status: 1, message: "central-link bundle is not the link entry" };
  if (conformanceMarkers.some((marker) => content.includes(marker)))
    return { status: 1, message: "central-link bundle contains conformance controls" };
  return { status: 0, message: "central-link bundle: link entry only, no conformance controls" };
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  const [mode] = arguments_;
  if (arguments_.length !== 1 || (mode !== "production" && mode !== "faults" && mode !== "link")) {
    console.error("usage: check-central-worker-bundle.ts {production,faults,link}");
    return 2;
  }
  const result = checkWorkerBundle(mode);
  console.log(result.message);
  return result.status;
}

if (import.meta.main) process.exitCode = main();
