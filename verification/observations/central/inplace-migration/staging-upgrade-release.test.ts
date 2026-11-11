import assert from "node:assert/strict";
import { randomUUIDv7 } from "node:crypto";
import { mkdtemp, writeFile, rename, rm, chmod, symlink } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { observeInstallationStopWindow } from "../../../machines/installation-stop-window.ts";
import { checkpointAndAwaitUpgradeRelease } from "./staging-upgrade-release.ts";

async function fixture(): Promise<{ directory: string; path: string; releaseId: string }> {
  const directory = await mkdtemp("/private/tmp/2026-11-s1-upgrade-release-test-");
  await chmod(directory, 0o700);
  return { directory, path: join(directory, "release.json"), releaseId: randomUUIDv7() };
}
async function release(path: string, releaseId: string): Promise<void> {
  await writeFile(`${path}.partial`, JSON.stringify({ kind: "releaseUpgrade", releaseId }), {
    mode: 0o600,
    flag: "wx",
  });
  await rename(`${path}.partial`, path);
}

test("owned children stop and checkpoint before the armed release; updater and replacement wait", async () => {
  const f = await fixture();
  const controller = new AbortController();
  const waiting = Promise.withResolvers<void>();
  const effects: string[] = [];
  const upgrading = observeInstallationStopWindow({
    stopOwnedChildren: async () => {
      effects.push("stopped");
    },
    observeStoppedOld: () =>
      checkpointAndAwaitUpgradeRelease({
        release: { path: f.path, releaseId: f.releaseId },
        signal: controller.signal,
        checkpoint: async () => {
          effects.push("checkpoint");
        },
        onWaiting: async () => {
          effects.push("armed");
          waiting.resolve();
        },
        onReleased: async () => {
          effects.push("released");
        },
      }),
    updateStoppedRoot: async () => {
      effects.push("updater");
    },
  }).then(() => {
    effects.push("replacement");
  });
  try {
    await waiting.promise;
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.deepEqual(effects, ["stopped", "checkpoint", "armed"]);
    await release(f.path, f.releaseId);
    await upgrading;
    assert.deepEqual(effects, [
      "stopped",
      "checkpoint",
      "armed",
      "released",
      "updater",
      "replacement",
    ]);
  } finally {
    controller.abort(new Error("test cleanup"));
    await upgrading.catch(() => {});
    await rm(f.directory, { recursive: true, force: true });
  }
});

test("cancelling an armed upgrade leaves both old children stopped without updater or replacement", async () => {
  const f = await fixture();
  const controller = new AbortController();
  const waiting = Promise.withResolvers<void>();
  const effects: string[] = [];
  const upgrading = observeInstallationStopWindow({
    stopOwnedChildren: async () => {
      effects.push("stopped");
    },
    observeStoppedOld: () =>
      checkpointAndAwaitUpgradeRelease({
        release: { path: f.path, releaseId: f.releaseId },
        signal: controller.signal,
        checkpoint: async () => {
          effects.push("checkpoint");
        },
        onWaiting: async () => {
          waiting.resolve();
        },
        onReleased: async () => {
          effects.push("released");
        },
      }),
    updateStoppedRoot: async () => {
      effects.push("updater");
    },
  }).then(() => {
    effects.push("replacement");
  });
  try {
    await waiting.promise;
    controller.abort(new Error("operator cancelled"));
    await assert.rejects(upgrading, /operator cancelled/);
    assert.deepEqual(effects, ["stopped", "checkpoint"]);
  } finally {
    controller.abort();
    await upgrading.catch(() => {});
    await rm(f.directory, { recursive: true, force: true });
  }
});

test("pre-existing release files are refused before the wait is announced", async () => {
  const f = await fixture();
  let armed = false;
  try {
    await release(f.path, f.releaseId);
    await assert.rejects(
      checkpointAndAwaitUpgradeRelease({
        release: { path: f.path, releaseId: f.releaseId },
        signal: new AbortController().signal,
        checkpoint: async () => {},
        onWaiting: async () => {
          armed = true;
        },
        onReleased: async () => {},
      }),
      /pre-existing/,
    );
    assert.equal(armed, false);
  } finally {
    await rm(f.directory, { recursive: true, force: true });
  }
});

test("cancel during checkpoint prevents arming, and missing or public directories are refused", async () => {
  const f = await fixture();
  const controller = new AbortController();
  let armed = false;
  try {
    await assert.rejects(
      checkpointAndAwaitUpgradeRelease({
        release: { path: f.path, releaseId: f.releaseId },
        signal: controller.signal,
        checkpoint: async () => {
          controller.abort(new Error("checkpoint cancelled"));
        },
        onWaiting: async () => {
          armed = true;
        },
        onReleased: async () => {},
      }),
      /checkpoint cancelled/,
    );
    assert.equal(armed, false);
    for (const path of [f.path.replace("/release.json", "-missing/release.json"), f.path]) {
      await chmod(f.directory, 0o755);
      await assert.rejects(
        checkpointAndAwaitUpgradeRelease({
          release: { path, releaseId: f.releaseId },
          signal: new AbortController().signal,
          checkpoint: async () => {},
          onWaiting: async () => {
            armed = true;
          },
          onReleased: async () => {},
        }),
        /ENOENT|private/,
      );
    }
    assert.equal(armed, false);
  } finally {
    await rm(f.directory, { recursive: true, force: true });
  }
});

test("release nonce, strict shape, permissions and symlinks are checked at the actual file event", async () => {
  for (const damage of ["nonce", "shape", "malformed", "public", "symlink"] as const) {
    const f = await fixture();
    const controller = new AbortController();
    const waiting = Promise.withResolvers<void>();
    let accepted = false;
    const pending = checkpointAndAwaitUpgradeRelease({
      release: { path: f.path, releaseId: f.releaseId },
      signal: controller.signal,
      checkpoint: async () => {},
      onWaiting: async () => {
        waiting.resolve();
      },
      onReleased: async () => {
        accepted = true;
      },
    });
    const rejected = assert.rejects(
      pending,
      /release|private|JSON|Unexpected|Expected|Unrecognized/,
    );
    try {
      await waiting.promise;
      if (damage === "symlink") {
        await writeFile(
          `${f.path}.source`,
          JSON.stringify({ kind: "releaseUpgrade", releaseId: f.releaseId }),
          { mode: 0o600 },
        );
        await symlink(`${f.path}.source`, f.path);
      } else {
        const body =
          damage === "malformed"
            ? "{"
            : JSON.stringify({
                kind: "releaseUpgrade",
                releaseId: damage === "nonce" ? randomUUIDv7() : f.releaseId,
                ...(damage === "shape" ? { extra: true } : {}),
              });
        await writeFile(`${f.path}.partial`, body, { mode: damage === "public" ? 0o644 : 0o600 });
        // Establish the invalid public mode even when the test process has a private umask.
        await chmod(`${f.path}.partial`, damage === "public" ? 0o644 : 0o600);
        await rename(`${f.path}.partial`, f.path);
      }
      await rejected;
      assert.equal(accepted, false, damage);
    } finally {
      controller.abort(new Error("test cleanup"));
      await pending.catch(() => {});
      await rm(f.directory, { recursive: true, force: true });
    }
  }
});
