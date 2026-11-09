/** Validated plans and shared facts for the isolated native-backed harness. */
import { lstat, readFile } from "node:fs/promises";
import { z } from "zod";
import { artifactSchema, fileSchema, type Session } from "../../declaration.ts";
import type { RouterMcp } from "../../clients/router-mcp.ts";
import type { RouterNative } from "../../clients/router-native.ts";
import type { OwnedProcess } from "../../machines/owned-process.ts";
import type { SessionGate, SessionGateObservation } from "../../access/session-gate.ts";
import type { Receipt } from "../../receipt.ts";
import { d1RuntimeDirectorySchema } from "./d1-runtime-allocation.ts";
import { d1TurnGrantSchema } from "./d1-turn-budget.ts";

export const d1HarnessSchema = z
  .object({
    suite: z.literal("d1-harness"),
    repository: z.string().startsWith("/"),
    driver: artifactSchema,
    cli: artifactSchema,
    native: fileSchema.extend({ version: z.string().min(1) }),
    evidenceDirectory: z.string().startsWith("/"),
    runtimeDirectory: d1RuntimeDirectorySchema.optional(),
    materialization: d1TurnGrantSchema.optional(),
    debugProxyOrigin: z.url().refine((value): boolean => {
      const url = new URL(value);
      return (
        url.protocol === "http:" &&
        url.hostname === "127.0.0.1" &&
        url.port !== "" &&
        !["19741", "19742", "8787", "8788"].includes(url.port) &&
        url.pathname === "/" &&
        url.username === "" &&
        url.password === "" &&
        url.search === "" &&
        url.hash === ""
      );
    }),
    deadline: z.iso.datetime(),
    queueScenario: z
      .enum(["none", "online-held", "offline-reopen", "online-timing"])
      .default("none"),
    twoProjects: z.boolean().default(false),
  })
  .strict()
  .refine(
    (declaration): boolean =>
      declaration.materialization === undefined ||
      (!declaration.twoProjects &&
        ["online-held", "online-timing"].includes(declaration.queueScenario) &&
        declaration.debugProxyOrigin === "http://127.0.0.1:43127" &&
        !(declaration.queueScenario === "online-timing" && declaration.materialization.reloadHeld)),
    "materialization is restricted to the declared Luna held/timing journeys",
  );
export type D1HarnessDeclaration = z.infer<typeof d1HarnessSchema>;

export interface D1Machine {
  readonly serviceDirectory: string;
  readonly gate: SessionGate;
  mcp: RouterMcp;
  native: RouterNative;
  readonly recipient: Session;
  nativeTurnCount(): number;
  stopForRestart(): Promise<void>;
  reopen(): Promise<void>;
}

/** Programmatic no-model journeys reuse the same owned Host/native lifecycle. */
export interface D1HarnessJourney {
  readonly declaration: unknown;
  readonly inboxAcceptedBarrierSocket?: string;
  centralStarted(central: OwnedProcess): void;
  sessionObservation(name: "a" | "b" | "c"): SessionGateObservation | undefined;
  connectionReady(input: {
    name: "a" | "b" | "c";
    serviceId: string;
    peer: string;
    gate: SessionGate;
    origin: string;
    centralRoot: string;
    receipt: Receipt;
  }): Promise<void>;
  recipientStarted(input: {
    native: RouterNative;
    recipient: Session;
    home: string;
    receipt: Receipt;
  }): Promise<void>;
  drive(input: {
    machines: readonly D1Machine[];
    receipt: Receipt;
    central: OwnedProcess;
    origin: string;
    centralRoot: string;
  }): Promise<void>;
  ownersStopped(input: { machines: readonly D1Machine[]; receipt: Receipt }): Promise<void>;
}

export async function readD1HarnessDeclaration(path: string): Promise<D1HarnessDeclaration> {
  const metadata = await lstat(path);
  if (
    !metadata.isFile() ||
    metadata.isSymbolicLink() ||
    metadata.uid !== process.getuid?.() ||
    (metadata.mode & 0o077) !== 0 ||
    metadata.size > 1024 * 1024
  )
    throw new Error("private bounded D1 declaration required");
  return d1HarnessSchema.parse(JSON.parse(await readFile(path, "utf8")));
}
