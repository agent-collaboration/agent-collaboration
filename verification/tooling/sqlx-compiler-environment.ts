import { delimiter, isAbsolute, join } from "node:path";
import { accessSync, constants } from "node:fs";

function requiredVariable(environment: NodeJS.ProcessEnv, name: string): string {
  const value = environment[name];
  if (value === undefined) throw new Error(`missing required environment variable ${name}`);
  return value;
}

export function compilerEnvironment(
  environment: NodeJS.ProcessEnv,
  kind: "package" | "sqlite",
): NodeJS.ProcessEnv {
  const prefix = kind === "package" ? "SQLX" : "SQLITE";
  const scoped: NodeJS.ProcessEnv = { ...environment, SQLX_OFFLINE: "true" };
  delete scoped.DATABASE_URL;
  delete scoped.SQLX_OFFLINE_DIR;
  if (
    environment.CARGO_PKG_NAME === requiredVariable(environment, `${prefix}_PREPARE_PACKAGE`) &&
    environment.CARGO_MANIFEST_DIR ===
      requiredVariable(environment, `${prefix}_PREPARE_MANIFEST_DIR`)
  ) {
    scoped.SQLX_OFFLINE = "false";
    scoped.DATABASE_URL = requiredVariable(environment, `${prefix}_PREPARE_DATABASE_URL`);
    if (kind === "package")
      scoped.SQLX_OFFLINE_DIR = requiredVariable(environment, "SQLX_PREPARE_METADATA_DIR");
    else if (environment.SQLX_OFFLINE_DIR !== undefined)
      scoped.SQLX_OFFLINE_DIR = environment.SQLX_OFFLINE_DIR;
  }
  return scoped;
}

export function executeCompiler(
  kind: "package" | "sqlite",
  arguments_: readonly string[] = process.argv.slice(2),
): number {
  const [compiler] = arguments_;
  if (compiler === undefined) {
    console.error(
      `usage: sqlx-${kind === "package" ? "package" : "sqlite"}-rustc-wrapper.ts <rustc> [arguments...]`,
    );
    return 2;
  }
  const environment = compilerEnvironment(process.env, kind);
  const candidates =
    isAbsolute(compiler) || compiler.includes("/")
      ? [compiler]
      : (environment.PATH ?? "/bin:/usr/bin")
          .split(delimiter)
          .map((directory) => join(directory, compiler));
  const executable = candidates.find((candidate): boolean => {
    try {
      accessSync(candidate, constants.X_OK);
      return true;
    } catch {
      return false;
    }
  });
  if (executable === undefined) throw new Error(`compiler executable not found: ${compiler}`);
  if (process.execve === undefined)
    throw new Error("SQLx compiler wrappers require Node.js with process.execve support");
  return process.execve(executable, arguments_, environment);
}
