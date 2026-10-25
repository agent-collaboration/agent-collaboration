import { dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { readFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { z } from "zod";
import { command, fingerprint } from "../machines/owned-process.ts";
import { decode } from "../exact-json.ts";
import type { Target } from "../declaration.ts";
import type { Receipt } from "../receipt.ts";
import { capturedCauseSchema } from "./error-cause.ts";

const inventorySchema = z.object({
  complete: z.literal(true),
  productPin: z.string(),
  pinnedVersionHealthy: z.boolean(),
  versions: z.array(z.unknown()),
  activeWeights: z.array(z.unknown()),
  rawNamespaces: z.array(z.unknown()),
});
type Inventory = z.infer<typeof inventorySchema>;
export type DeployOutcome =
  | { readonly kind: "captureIncomplete"; readonly inventory: Inventory }
  | { readonly kind: "deployed"; readonly inventory: Inventory }
  | { readonly kind: "failedReconciled"; readonly inventory: Inventory }
  | { readonly kind: "effectUnknown"; readonly reason: string };
export class StagingDeploy {
  readonly declaration: Extract<Target, { kind: "staging" }>["deploy"];
  readonly receipt: Receipt;
  readonly productPin: string;
  constructor(
    declaration: Extract<Target, { kind: "staging" }>["deploy"],
    receipt: Receipt,
    productPin: string,
  ) {
    this.declaration = declaration;
    this.receipt = receipt;
    this.productPin = productPin;
  }
  private async inventory(): Promise<Inventory> {
    const result = await command(
      this.declaration.executable.path,
      this.declaration.inventoryArguments,
      { timeoutSeconds: 60, cwd: this.declaration.cwd },
    );
    if (result.exit !== 0) throw new Error("deployment inventory unavailable");
    const inventory = inventorySchema.parse(decode(result.stdout));
    if (inventory.productPin !== this.productPin) throw new Error("inventory pin differs");
    return inventory;
  }
  async run(): Promise<DeployOutcome> {
    if (
      (await fingerprint(this.declaration.executable.path)) !==
        this.declaration.executable.sha256 ||
      (await fingerprint(this.declaration.cfBundle.path)) !== this.declaration.cfBundle.sha256
    )
      throw new Error("deploy executable/client input changed");
    const invocation = randomUUID();
    const before = await this.inventory();
    await this.receipt.save(`deployment-${invocation}-before.json`, before);
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      const preload = fileURLToPath(new URL("./capture-cause.ts", import.meta.url));
      const environment = {
        ...process.env,
        PATH: `${dirname(process.execPath)}:${process.env.PATH ?? ""}`,
        NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ""} --import=${pathToFileURL(preload).href}`,
        HARNESS_CAUSE_OUTPUT: `${this.receipt.directory}/deploy-${invocation}-${attempt}-causes.jsonl`,
        HARNESS_CF_BUNDLE: this.declaration.cfBundle.path,
        HARNESS_CF_FACTORY: this.declaration.undiciFactoryExport,
      };
      // Keep credentials and unstructured client output in memory, out of receipts.
      const { spawn } = await import("node:child_process");
      const child = spawn(this.declaration.executable.path, this.declaration.arguments, {
        cwd: this.declaration.cwd,
        env: environment,
        stdio: ["ignore", "ignore", "ignore"],
        signal: AbortSignal.timeout(this.declaration.timeoutSeconds * 1000),
      });
      let exit: number | null = null;
      try {
        exit = await new Promise<number | null>((resolve, reject): void => {
          child.once("error", reject);
          child.once("close", resolve);
        });
      } catch {
        exit = null;
      }
      let after: Inventory;
      try {
        after = await this.inventory();
      } catch {
        return { kind: "effectUnknown", reason: "failed to reconcile full inventory" };
      }
      await this.receipt.save(`deployment-${invocation}-after-${attempt}.json`, after);
      let causes: z.infer<typeof capturedCauseSchema>[] = [];
      try {
        causes = (await readFile(environment.HARNESS_CAUSE_OUTPUT, "utf8"))
          .trim()
          .split("\n")
          .map((line): z.infer<typeof capturedCauseSchema> =>
            capturedCauseSchema.parse(JSON.parse(line)),
          );
      } catch {
        /* A pre-request failure remains unknown. */
      }
      await this.receipt.record("deployAttempt", {
        attempt,
        invocation,
        exit,
        stage:
          exit === 0
            ? "completed"
            : causes.length === 0
              ? "client-failed-without-fetch-capture"
              : "captured-fetch-failure",
        causes,
      });
      if (exit === null)
        return {
          kind: "effectUnknown",
          reason: "deployer interrupted; a snapshot cannot establish that all effects have settled",
        };
      if (
        causes.some((entry): boolean => entry.kind === "limit") ||
        (exit !== 0 && causes.length === 0)
      )
        return { kind: "captureIncomplete", inventory: after };
      if (exit === 0 && after.pinnedVersionHealthy) return { kind: "deployed", inventory: after };
      const unchanged =
        isDeepStrictEqual(before.versions, after.versions) &&
        isDeepStrictEqual(before.activeWeights, after.activeWeights) &&
        isDeepStrictEqual(before.rawNamespaces, after.rawNamespaces);
      if (!unchanged)
        return {
          kind: "effectUnknown",
          reason: "deployment failure changed version, weight or raw namespace inventory",
        };
      if (attempt === 2 || !after.pinnedVersionHealthy)
        return { kind: "failedReconciled", inventory: after };
    }
    return { kind: "effectUnknown", reason: "deployment did not settle" };
  }
}
