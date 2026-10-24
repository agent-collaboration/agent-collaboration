import type { Remote } from "./remote.ts";
import { command } from "./owned-process.ts";

export async function sampleConfigAccess(input: {
  remote: Remote | undefined;
  paths: readonly string[];
  ownedPids: readonly number[];
}): Promise<unknown> {
  const scope = {
    ownedProcessCount: input.ownedPids.length,
    writeAttribution: "notEstablished",
    limitation: "Open descriptors are sampled; filesystem writes are not syscall-traced.",
  };
  if (input.ownedPids.length === 0) return { ...scope, status: "notSampled", openFiles: [] };
  const arguments_ = ["-nP", "-a", "-p", input.ownedPids.join(","), "-Fpfan", ...input.paths];
  try {
    const result =
      input.remote === undefined
        ? await command("/usr/sbin/lsof", arguments_, { timeoutSeconds: 5 })
        : await input.remote.output(["/usr/sbin/lsof", ...arguments_], { timeoutSeconds: 5 });
    if (![0, 1].includes(result.exit ?? -1) || result.stderr !== "")
      return { ...scope, status: "unavailable" };
    const openFiles: { pid: number; file: number; access: string }[] = [];
    let pid = 0;
    let access = "unknown";
    for (const line of result.stdout.split("\n")) {
      if (line.startsWith("p")) pid = Number(line.slice(1));
      if (line.startsWith("f")) access = "unknown";
      if (line.startsWith("a")) access = line.slice(1);
      if (line.startsWith("n")) {
        const file = input.paths.indexOf(line.slice(1));
        if (!input.ownedPids.includes(pid) || file < 0) return { ...scope, status: "unavailable" };
        openFiles.push({ pid, file, access });
      }
    }
    return { ...scope, status: "sampled", openFiles };
  } catch {
    return { ...scope, status: "unavailable" };
  }
}
