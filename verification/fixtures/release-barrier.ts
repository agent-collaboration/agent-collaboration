// A fixture's release barrier: a test releases a held step by creating a file
// next to the fixture's receipt, then ringing the fixture's doorbell (a file
// beside the receipt it appends to). The fixture wakes on watches, with no
// timer.
//
// A folder watch alone can't be trusted on macOS: it is backed by FSEvents,
// which can start late and can drop or coalesce an event outright, so a
// release file's creation may never be reported while a turn is held and
// nothing else touches the folder (the D1 send-race hang). The doorbell is an
// existing file the fixture creates at launch and watches directly; a file
// watch on macOS is kqueue, which reports writes to that file without losing
// them. The test rings it only after the release file exists, so any event it
// delivers finds the file. The folder watch stays as a second wake source,
// and every waiter checks for its file when it registers.
import { closeSync, existsSync, openSync, watch } from "node:fs";
import { dirname } from "node:path";

class ReleaseBarrier {
  readonly #waiters = new Set<() => void>();

  constructor(folder: string, doorbell: string | undefined) {
    this.#wakeOn(folder);
    if (doorbell !== undefined) {
      // Created if missing, never truncated: a test may ring it first.
      closeSync(openSync(doorbell, "a"));
      this.#wakeOn(doorbell);
    }
  }

  #wakeOn(path: string): void {
    const watcher = watch(path);
    // The fixture's stdin keeps it alive; a watch alone never does.
    watcher.unref();
    watcher.on("change", (): void => {
      for (const check of this.#waiters) check();
    });
    watcher.on("error", (error: Error): void => {
      throw error;
    });
  }

  wait(path: string): Promise<void> {
    return new Promise<void>((resolve): void => {
      const check = (): void => {
        if (!existsSync(path)) return;
        this.#waiters.delete(check);
        resolve();
      };
      this.#waiters.add(check);
      check();
    });
  }
}

const barriers = new Map<string, ReleaseBarrier>();

/**
 * Starts the release barrier beside `receiptPath`: its folder watch and its
 * doorbell, `<receiptPath>.doorbell`. Call when the fixture starts.
 */
export function startReleaseBarrier(receiptPath: string): void {
  const folder = dirname(receiptPath);
  if (!barriers.has(folder))
    barriers.set(folder, new ReleaseBarrier(folder, `${receiptPath}.doorbell`));
}

/** Resolves once the file at `path` exists. */
export function fileAppears(path: string): Promise<void> {
  const folder = dirname(path);
  let barrier = barriers.get(folder);
  if (barrier === undefined) {
    barrier = new ReleaseBarrier(folder, undefined);
    barriers.set(folder, barrier);
  }
  return barrier.wait(path);
}
