// The Router's PreToolUse hook for a Claude session it started in auto
// (D1 R18). Claude runs it before a Write, Edit, MultiEdit, NotebookEdit or
// Bash call, with the call as JSON on stdin. A call that names a path
// outside the session's work roots is answered "ask", so Claude asks the
// Router (and the session's approver) instead of letting its own classifier
// decide; anything else gets no answer, and Claude's own auto mode and the
// owner's settings decide as usual.
//
// argv[2] is a JSON array of the Router's work roots for the session. A path
// is inside when, resolved through symlinks, it lies under one of them or
// under an additionalDirectories entry of the owner's or the project's own
// Claude settings (the Router adds to those grants, never replaces them).
// Input this hook can't read for certain is answered "ask".

import { existsSync, lstatSync, readFileSync, realpathSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";

type HookAnswer = { readonly ask: false } | { readonly ask: true; readonly reason: string };

const DEVICE_PATHS = ["/dev/null", "/dev/stdin", "/dev/stdout", "/dev/stderr", "/dev/tty"];
const SHELL_OPERATORS = new Set([";", "|", "&", "<", ">", "(", ")", "\n"]);
/// Operators after which the next word is a new command.
const COMMAND_SEPARATORS = new Set([";", "|", "&", "(", ")", "\n"]);
/// Programs that only read and print, run from any folder without asking
/// when nothing is redirected (D1 R18: a write or a shell command outside
/// asks; these write nothing).
const READ_ONLY_PROGRAMS = new Set([
  "ls", "cat", "head", "tail", "wc", "grep", "rg", "stat", "file", "pwd", "echo",
  "printf", "which", "true", "false", "test", "du", "cd",
]);

/// One shell token: a word, or an operator character.
type ShellToken = { readonly kind: "word"; readonly text: string } | { readonly kind: "operator"; readonly text: string };

function main(): void {
  let answer: HookAnswer;
  try {
    answer = decide(process.argv[2], readFileSync(0, "utf8"));
  } catch (error) {
    answer = { ask: true, reason: `the Router's work-roots hook could not read the call: ${String(error)}` };
  }
  if (answer.ask) {
    process.stdout.write(
      JSON.stringify({
        hookSpecificOutput: {
          hookEventName: "PreToolUse",
          permissionDecision: "ask",
          permissionDecisionReason: answer.reason,
        },
      }),
    );
  }
}

function decide(rootsArgument: string | undefined, stdin: string): HookAnswer {
  const routerRoots: unknown = JSON.parse(rootsArgument ?? "");
  if (!Array.isArray(routerRoots) || !routerRoots.every((root) => typeof root === "string"))
    return { ask: true, reason: "the Router's work-roots hook was started without its roots" };
  const call: unknown = JSON.parse(stdin);
  if (!isRecord(call)) return { ask: true, reason: "the tool call is not a JSON object" };
  const cwd = typeof call.cwd === "string" && isAbsolute(call.cwd) ? call.cwd : undefined;
  if (cwd === undefined) return { ask: true, reason: "the tool call names no absolute working folder" };
  const roots = [...routerRoots, ...ownedAdditionalDirectories(cwd)]
    .map(canonical)
    .filter((root): root is string => root !== undefined);
  const input = isRecord(call.tool_input) ? call.tool_input : {};
  const tool = String(call.tool_name ?? "");
  const paths = namedPaths(tool, input, cwd);
  if (paths === undefined) return { ask: true, reason: "the command can't be read for certain" };
  const resolvedCwd = canonical(cwd);
  const cwdInside = resolvedCwd !== undefined && roots.some((root) => under(resolvedCwd, root));
  if (tool === "Bash" && !cwdInside && commandCouldWrite(String(input.command)))
    return { ask: true, reason: `the command runs in ${cwd}, outside the Router's work roots for this session` };
  const outside = paths.find((path) => {
    if (DEVICE_PATHS.includes(path)) return false;
    const resolved = canonical(path);
    return resolved === undefined || !roots.some((root) => under(resolved, root));
  });
  return outside === undefined
    ? { ask: false }
    : { ask: true, reason: `${outside} is outside the Router's work roots for this session` };
}

/// The paths a call names, resolved against the working folder; undefined
/// when a command can't be split for certain.
function namedPaths(
  tool: string,
  input: Record<string, unknown>,
  cwd: string,
): string[] | undefined {
  switch (tool) {
    case "Write":
    case "Edit":
    case "MultiEdit":
      return typeof input.file_path === "string" ? [resolve(cwd, input.file_path)] : undefined;
    case "NotebookEdit":
      return typeof input.notebook_path === "string"
        ? [resolve(cwd, input.notebook_path)]
        : undefined;
    case "Bash": {
      if (typeof input.command !== "string") return undefined;
      const tokens = shellTokens(input.command);
      if (tokens === undefined) return undefined;
      const paths: string[] = [];
      for (const operand of operands(tokens)) {
        const candidate = pathCandidate(operand);
        if (candidate === "unreadable") return undefined;
        if (candidate !== undefined) paths.push(resolve(cwd, candidate));
      }
      return paths;
    }
    default:
      return [];
  }
}

/// Every word a command could take as a path: each operand and redirection
/// target, and the value of an option written --flag=value. A command's own
/// name, a NAME=value assignment before it, and a bare option are left out.
function operands(tokens: readonly ShellToken[]): string[] {
  const found: string[] = [];
  let expectingCommand = true;
  for (const token of tokens) {
    if (token.kind === "operator") {
      if (COMMAND_SEPARATORS.has(token.text)) expectingCommand = true;
      continue;
    }
    if (expectingCommand) {
      if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(token.text)) continue;
      expectingCommand = false;
      // A command named by a path is still a path it runs.
      if (token.text.includes("/")) found.push(token.text);
      continue;
    }
    if (token.text.startsWith("-") && !token.text.includes("=")) continue;
    found.push(token.text);
  }
  return found;
}

/// Whether a command could write: any redirection into a file, or any
/// program that isn't one of the read-only ones. Read for certain only;
/// anything unreadable was already answered ask.
function commandCouldWrite(command: string): boolean {
  const tokens = shellTokens(command) ?? [];
  let expectingCommand = true;
  for (const [index, token] of tokens.entries()) {
    if (token.kind === "operator") {
      if (token.text === ">") return true;
      if (COMMAND_SEPARATORS.has(token.text)) expectingCommand = true;
      continue;
    }
    if (!expectingCommand) continue;
    if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(token.text)) continue;
    expectingCommand = false;
    if (!READ_ONLY_PROGRAMS.has(token.text)) return true;
    if (index > 0 && tokens[index - 1]?.text === ">") return true;
  }
  return false;
}

/// A word that names a path, with ~ and environment variables expanded; the
/// value of a --flag=value word counts. "unreadable" when an expansion can't
/// be resolved here. A bare name is a path in the working folder.
function pathCandidate(word: string): string | undefined | "unreadable" {
  const value = word.startsWith("-") && word.includes("=") ? word.slice(word.indexOf("=") + 1) : word;
  if (value.includes("://") || value === "") return undefined;
  const expanded = expand(value);
  if (expanded === undefined) return "unreadable";
  // A glob is judged by the folder it searches.
  const glob = expanded.search(/[*?[]/);
  return glob < 0 ? expanded : dirname(expanded.slice(0, glob) + "x");
}

function expand(value: string): string | undefined {
  let expanded = value.startsWith("~/") || value === "~" ? homedir() + value.slice(1) : value;
  expanded = expanded.replace(/\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)/g, (_, braced, bare) => {
    const name = braced ?? bare;
    return process.env[name] ?? `\u0000${name}`;
  });
  return expanded.includes("\u0000") || expanded.includes("$") || expanded.includes("`")
    ? undefined
    : expanded;
}

/// Splits a command into words the way a shell would for quoting, keeping
/// its operators as tokens; undefined for an unterminated quote.
function shellTokens(command: string): ShellToken[] | undefined {
  const words: ShellToken[] = [];
  let word = "";
  let started = false;
  for (let index = 0; index < command.length; index += 1) {
    const character = command[index];
    if (character === "'") {
      const end = command.indexOf("'", index + 1);
      if (end < 0) return undefined;
      word += command.slice(index + 1, end);
      started = true;
      index = end;
    } else if (character === '"') {
      let end = index + 1;
      while (end < command.length && command[end] !== '"') end += command[end] === "\\" ? 2 : 1;
      if (end >= command.length) return undefined;
      word += command.slice(index + 1, end).replace(/\\(.)/g, "$1");
      started = true;
      index = end;
    } else if (character === "\\" && index + 1 < command.length) {
      word += command[index + 1];
      started = true;
      index += 1;
    } else if (/\s/.test(character) || SHELL_OPERATORS.has(character)) {
      if (started) words.push({ kind: "word", text: word });
      if (SHELL_OPERATORS.has(character)) words.push({ kind: "operator", text: character });
      word = "";
      started = false;
    } else {
      word += character;
      started = true;
    }
  }
  if (started) words.push({ kind: "word", text: word });
  return words;
}

/// The additionalDirectories the owner's and the project's own Claude
/// settings grant: the Router adds to them and never drops one.
function ownedAdditionalDirectories(cwd: string): string[] {
  const home = homedir();
  const sources: Array<[string, string]> = [
    [join(home, ".claude", "settings.json"), home],
    [join(home, ".claude", "settings.local.json"), home],
    [join(cwd, ".claude", "settings.json"), cwd],
    [join(cwd, ".claude", "settings.local.json"), cwd],
  ];
  const directories: string[] = [];
  for (const [file, base] of sources) {
    if (!existsSync(file)) continue;
    let settings: unknown;
    try {
      settings = JSON.parse(readFileSync(file, "utf8"));
    } catch {
      continue;
    }
    const permissions = isRecord(settings) ? settings.permissions : undefined;
    const granted = isRecord(permissions) ? permissions.additionalDirectories : undefined;
    if (!Array.isArray(granted)) continue;
    for (const entry of granted) {
      if (typeof entry !== "string") continue;
      const expanded = expand(entry);
      if (expanded !== undefined) directories.push(resolve(base, expanded));
    }
  }
  return directories;
}

/// The path through every symlink in its existing part, with the absent rest
/// appended, so a file about to be written resolves through its folder.
/// Undefined when something present can't be resolved (a dangling or
/// unreadable symlink): a write would follow it to a place this can't see.
function canonical(path: string): string | undefined {
  const absent: string[] = [];
  let existing = resolve(path);
  for (;;) {
    try {
      return [realpathSync(existing), ...absent.reverse()].reduce((left, right) => join(left, right));
    } catch {
      if (isPresent(existing)) return undefined;
      const parent = dirname(existing);
      if (parent === existing) return undefined;
      absent.push(basename(existing));
      existing = parent;
    }
  }
}

/// Whether a directory entry exists, even one that is a dangling symlink.
function isPresent(path: string): boolean {
  try {
    lstatSync(path);
    return true;
  } catch {
    return false;
  }
}

function under(path: string, root: string): boolean {
  return path === root || path.startsWith(root.endsWith("/") ? root : `${root}/`);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

main();
