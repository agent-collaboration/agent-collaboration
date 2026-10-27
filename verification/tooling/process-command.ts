import { constants } from "node:os";
import { spawnSync } from "node:child_process";

export interface CommandOptions {
  readonly cwd?: string;
  readonly env?: NodeJS.ProcessEnv;
  readonly inherit?: boolean;
}

export interface CommandResult {
  readonly status: number;
  readonly stdout: string;
  readonly stderr: string;
}

export type CommandRunner = (command: readonly string[], options: CommandOptions) => CommandResult;

export const runProcess: CommandRunner = (command, options): CommandResult => {
  const [executable, ...arguments_] = command;
  if (executable === undefined) throw new Error("command must name an executable");
  const result = spawnSync(executable, arguments_, {
    ...options,
    encoding: "utf8",
    stdio: options.inherit ? "inherit" : ["inherit", "pipe", "pipe"],
    maxBuffer: Number.MAX_SAFE_INTEGER,
  });
  if (result.error !== undefined) throw result.error;
  return {
    status: result.status ?? (result.signal === null ? 1 : -constants.signals[result.signal]),
    stdout: (result.stdout ?? "").replace(/\r\n|\r/g, "\n"),
    stderr: (result.stderr ?? "").replace(/\r\n|\r/g, "\n"),
  };
};

export function runCommands(
  commands: readonly (readonly string[])[],
  options: CommandOptions,
  runner: CommandRunner,
): number {
  for (const command of commands) {
    const result = runner(command, options);
    if (result.status !== 0) return result.status;
  }
  return 0;
}

export function runChecked(
  command: readonly string[],
  options: CommandOptions = {},
  runner: CommandRunner = runProcess,
): string {
  console.log(`$ ${command.join(" ")}`);
  const result = runner(command, options);
  if (result.status !== 0)
    throw new Error(`${command.join(" ")} exited ${result.status}: ${result.stderr.trim()}`);
  return result.stdout.trim();
}
