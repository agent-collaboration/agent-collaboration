import { randomUUID } from "node:crypto";
import {
  chmodSync,
  closeSync,
  fchmodSync,
  mkdtempSync,
  openSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";

interface SettingsFileInputs {
  readonly temporaryRoot?: string;
  readonly writeContents?: (descriptor: number, contents: string) => void;
}

/** Immutable derived settings remain available for the provider process lifetime. */
export class SettingsFiles {
  readonly #inputs: SettingsFileInputs;
  readonly #ownedPaths = new Set<string>();
  #directory: string | undefined;

  constructor(inputs: SettingsFileInputs = {}) {
    this.#inputs = inputs;
  }

  writeSettings(contents: string): string {
    let descriptor: number | undefined;
    let path: string | undefined;
    try {
      if (this.#directory === undefined) {
        const root = this.#inputs.temporaryRoot ?? tmpdir();
        if (!isAbsolute(root)) throw new Error("Temporary root must be absolute");
        this.#directory = mkdtempSync(join(root, "codex-claude-settings-"));
        chmodSync(this.#directory, 0o700);
      }
      path = join(this.#directory, `${randomUUID()}.json`);
      descriptor = openSync(path, "wx", 0o600);
      this.#ownedPaths.add(path);
      fchmodSync(descriptor, 0o600);
      if (this.#inputs.writeContents) this.#inputs.writeContents(descriptor, contents);
      else writeFileSync(descriptor, contents, "utf8");
      closeSync(descriptor);
      descriptor = undefined;
      return path;
    } catch {
      let cleanupFailed = false;
      if (descriptor !== undefined) {
        try {
          closeSync(descriptor);
        } catch {
          cleanupFailed = true;
        }
      }
      if (path !== undefined && this.#ownedPaths.has(path)) {
        try {
          unlinkSync(path);
          this.#ownedPaths.delete(path);
        } catch {
          cleanupFailed = true;
        }
      }
      // Retain any exact partial path in bookkeeping if removal failed.
      throw new Error(
        cleanupFailed
          ? "Qualified Claude settings file failed: PartialCleanup"
          : "Qualified Claude settings file failed: Creation",
      );
    }
  }
}
