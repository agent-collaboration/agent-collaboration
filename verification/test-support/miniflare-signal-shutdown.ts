// Standalone bridges own one Miniflare and await its disposal before signal exit.
// In-process loaders must not use this process-level policy.
import type { Miniflare } from "miniflare";

const signals = [
  { name: "SIGINT", exitCode: 130 },
  { name: "SIGTERM", exitCode: 143 },
  { name: "SIGHUP", exitCode: 129 },
] as const;

export async function constructMiniflareWithOwnedSignalShutdown(
  construct: () => Miniflare,
): Promise<Miniflare> {
  const before = signals.map(({ name }) => new Set(process.listeners(name)));
  const runtime = construct();
  const disposeRuntime = runtime.dispose.bind(runtime);
  let pendingDisposal: Promise<void> | undefined;
  runtime.dispose = (): Promise<void> => (pendingDisposal ??= disposeRuntime());
  const added = signals.map(({ name }, index) =>
    process.listeners(name).filter((listener) => !before[index]?.has(listener)),
  );
  for (const [{ name }, listeners] of signals.map(
    (signal, index) => [signal, added[index] ?? []] as const,
  )) {
    for (const listener of listeners) process.removeListener(name, listener);
  }
  const missing = signals.filter((_, index) => added[index]?.length === 0);
  if (missing.length !== 0) {
    const failure = new Error(
      `Standalone Miniflare did not add expected signal hooks: ${missing.map(({ name }) => name).join(", ")}`,
    );
    try {
      await runtime.dispose();
    } catch (cleanup: unknown) {
      throw new AggregateError([failure, cleanup], "Signal ownership and cleanup failed", {
        cause: failure,
      });
    }
    throw failure;
  }
  let stopping = false;
  for (const { name, exitCode } of signals) {
    process.on(name, (): void => {
      if (stopping) return;
      stopping = true;
      void runtime.dispose().then(
        (): never => process.exit(exitCode),
        (): never => {
          process.stderr.write("Standalone Miniflare signal cleanup failed\n");
          process.exit(1);
        },
      );
    });
  }
  return runtime;
}
