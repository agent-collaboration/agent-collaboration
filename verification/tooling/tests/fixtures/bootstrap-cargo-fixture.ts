#!/usr/bin/env node
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join } from "node:path";
import { z } from "zod";

function main(): number {
  const executable = z.string().parse(process.argv[1]);
  const arguments_ = process.argv.slice(2);
  if (basename(executable) === "cargo-nextest") {
    if (arguments_[0] !== "nextest") return 2;
    console.log(`cargo-nextest ${readFileSync(join(dirname(executable), "version"), "utf8")}`);
    return 0;
  }
  appendFileSync(z.string().parse(process.env.INSTALL_CALLS), `${arguments_.join(" ")}\n`);
  if (process.env.FAIL_INSTALL === "1") return 9;
  const root = z.string().parse(arguments_[arguments_.indexOf("--root") + 1]);
  mkdirSync(join(root, "bin"), { recursive: true });
  const binary = join(root, "bin/cargo-nextest");
  if (existsSync(binary)) unlinkSync(binary);
  symlinkSync(new URL(import.meta.url), binary);
  writeFileSync(join(root, "bin/version"), process.env.INSTALLED_VERSION ?? "0.9.137");
  return 0;
}

if (import.meta.main) process.exitCode = main();
