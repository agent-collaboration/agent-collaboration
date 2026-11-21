// D1 runtime adaptation keeps a short private app-server socket and checks
// the canonical lifecycle's current proxy database and qualified Claude launch.
import { existsSync, lstatSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, isAbsolute } from "node:path";
import { spawnSync } from "node:child_process";
import { z } from "zod";
import { runLifecycle } from "../../runtime/owned-runtime.ts";
import {
  launchRuntimeChild,
  loadRuntimePlan,
  type RuntimeLaunch,
} from "../../runtime/runtime-plan.ts";
import { verifyReadyPost261 as verifyReady } from "./runtime-readiness.ts";
import { encode } from "../../../exact-json.ts";

function pathsInside(root: string): string[] {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry): string[] => {
    const path = join(root, entry.name);
    return entry.isDirectory() ? [path, ...pathsInside(path)] : [path];
  });
}
function checkConvertedRootConsumers(root: string, allowedRouter: number | null = null): void {
  const status = statSync(root);
  if (
    lstatSync(root).isSymbolicLink() ||
    status.uid !== process.getuid?.() ||
    (status.mode & 0o022) !== 0
  )
    throw new Error("debug root ownership mismatch");
  if (existsSync(join(root, "secrets/debug-plaintext.marker")))
    throw new Error("plaintext debug secrets forbidden");
  for (const path of pathsInside(root)) {
    if (path.endsWith(".sock")) throw new Error("debug root still publishes another Host socket");
    if (
      !statSync(path, { throwIfNoEntry: false })?.isFile() ||
      !path.split("/").at(-1)?.includes(".sqlite")
    )
      continue;
    const read = spawnSync("/usr/sbin/lsof", ["-t", "--", path], {
      encoding: "utf8",
      timeout: 5000,
    });
    if (read.error !== undefined) throw read.error;
    if (read.status !== 0 && read.status !== 1) throw new Error("database consumer lookup failed");
    for (const processId of read.stdout.split(/\s+/).filter(Boolean)) {
      if (
        Number(processId) !== allowedRouter ||
        !path.startsWith(`${root}/provider-proxy.sqlite`) ||
        path.slice(root.length + 1).includes("/")
      )
        throw new Error("debug root has an unowned consumer");
    }
  }
  if (existsSync(join(root, "agent-communication/service.json")))
    throw new Error("debug root still publishes another Host");
}
async function main(): Promise<void> {
  if (process.argv.includes("--help")) {
    console.log(
      "usage: owned-auto-runtime.ts PLAN.json [--check]\nUses the canonical owning runtime with the converted provider-proxy.sqlite path. Requires the explicit root slot and signed declared build. --check runs existing read-only readiness.",
    );
    return;
  }
  const planPath = process.argv[2];
  if (planPath === undefined) throw new Error("runtime plan required");
  const plan = loadRuntimePlan(planPath);
  const socketDirectory = z
    .object({ directory: z.string().refine(isAbsolute), mode: z.literal("0700") })
    .parse(
      JSON.parse(readFileSync(join(dirname(planPath), "socket-directory.json"), "utf8")),
    ).directory;
  const socketStatus = statSync(socketDirectory);
  if (
    lstatSync(socketDirectory).isSymbolicLink() ||
    socketStatus.uid !== process.getuid?.() ||
    (socketStatus.mode & 0o077) !== 0
  )
    throw new Error("private socket directory ownership mismatch");
  if (process.argv.includes("--check")) {
    console.log(encode(await verifyReady(plan)));
    return;
  }
  if (
    !existsSync(join(plan.router_root, "provider-proxy.sqlite")) ||
    existsSync(join(plan.router_root, "state.sqlite"))
  )
    throw new Error("expected converted root layout is absent; no repair attempted");
  await runLifecycle(plan, {
    checkConsumers: checkConvertedRootConsumers,
    launch: async (input: RuntimeLaunch) => {
      return launchRuntimeChild(adaptRuntimeLaunch({ input, plan, socketDirectory }));
    },
  });
}
if (import.meta.main) await main();

/** Keep canonical lifecycle ownership while adapting only the current layout/launch. */
export function adaptRuntimeLaunch(props: {
  readonly input: RuntimeLaunch;
  readonly plan: Pick<
    import("../../runtime/runtime-plan.ts").RuntimePlan,
    "router_root" | "claude_adapter"
  >;
  readonly socketDirectory: string;
}): RuntimeLaunch {
  const { input, plan, socketDirectory } = props;
  const command = [...input.command];
  if (command[1] === "serve") {
    const stateIndex = command.indexOf("--state-db");
    if (
      stateIndex < 0 ||
      command[stateIndex + 1] !== join(plan.router_root, "provider-proxy.sqlite")
    )
      throw new Error("canonical runtime proxy command changed");
  }
  if (command[1] === "host") {
    const executableIndex = command.indexOf("--claude-acp-executable");
    if (
      executableIndex < 0 ||
      command[executableIndex + 1] !== plan.claude_adapter ||
      command.includes("--claude-acp-arguments")
    )
      throw new Error("canonical runtime Claude launch changed");
  }
  const environment =
    command[1] === "host"
      ? {
          ...input.environment,
          CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET: join(socketDirectory, "app.sock"),
        }
      : input.environment;
  return {
    ...input,
    command,
    ...(environment === undefined ? {} : { environment }),
  };
}
