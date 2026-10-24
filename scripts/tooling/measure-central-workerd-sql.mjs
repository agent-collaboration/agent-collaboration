// Pass the same pinned Miniflare entrypoint used by central's runtime harness.
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { readdirSync, readFileSync } from "node:fs";

const [miniflareEntrypoint] = process.argv.slice(2);
if (!miniflareEntrypoint) throw new Error("pinned Miniflare entrypoint required");
const { Miniflare, convertV4MiniflareOptions } = await import(pathToFileURL(resolve(miniflareEntrypoint)).href);
const runtime = new Miniflare(convertV4MiniflareOptions({
  cf: false,
  workers: [{
    name: "central-sql-limit-probe",
    compatibilityDate: "2026-08-21",
    modules: [{ type: "ESModule", path: resolve("scripts/tooling/central-sql-limit-worker.mjs") }],
    durableObjects: { SQL_LIMIT_PROBE: { className: "SqlLimitProbe", useSQLite: true } },
  }],
}));
try {
  await runtime.ready;
  const response = await runtime.dispatchFetch("http://sql-limit-probe/");
  if (!response.ok) throw new Error(`probe HTTP status ${response.status}`);
  const observations = await response.json();
  const firstFailure = observations.find((item) => !item.accepted);
  if (!firstFailure || firstFailure.terms === 1 ||
      observations.some((item) => item.terms > firstFailure.terms && item.accepted) ||
      observations.some((item) => !item.accepted && !item.error.includes("too many terms in compound SELECT"))) {
    throw new Error(`probe did not establish a contiguous compound-term limit: ${JSON.stringify(observations)}`);
  }
  const maximumCompoundTerms = firstFailure.terms - 1;
  const sqlRoot = resolve("crates/collaboration-central-core/sql");
  const compoundReaders = [];
  for (const file of readdirSync(sqlRoot, { recursive: true }).filter((file) => file.endsWith(".sql"))) {
    // These checked statements have flat compound readers. Count conservatively
    // across nested subqueries too, so growing them cannot evade this guard.
    const sql = readFileSync(resolve(sqlRoot, file), "utf8")
      .replace(/--[^\n]*|\/\*[\s\S]*?\*\//g, "")
      .replace(/'(?:''|[^'])*'/g, "''");
    const terms = 1 + [...sql.matchAll(/\b(?:UNION|INTERSECT|EXCEPT)\b/gi)].length;
    if (terms > 1) compoundReaders.push({ file, terms });
    if (terms > maximumCompoundTerms) {
      throw new Error(`${file} uses ${terms} compound terms; measured Workerd maximum is ${maximumCompoundTerms}`);
    }
  }
  console.log(JSON.stringify({ maximumCompoundTerms, observations, compoundReaders }, null, 2));
} finally {
  await runtime.dispose();
}
