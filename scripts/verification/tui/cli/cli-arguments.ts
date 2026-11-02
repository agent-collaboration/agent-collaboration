import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import type { ParseArgsOptionsConfig } from 'node:util';
export class CliUsageError extends Error {}
export function usageFailure(message: string): never {
	throw new CliUsageError(message);
}
export function strictArguments<Options extends ParseArgsOptionsConfig>(
	args: readonly string[],
	options: Options,
	allowPositionals: boolean,
): ReturnType<
	typeof parseArgs<{ args: string[]; options: Options; strict: true; allowPositionals: boolean }>
> {
	try {
		return parseArgs({ args: [...args], options, strict: true, allowPositionals });
	} catch (error) {
		return usageFailure(error instanceof Error ? error.message : String(error));
	}
}
export function planArgument(positionals: readonly string[]): string {
	if (positionals.length !== 1 || positionals[0] === undefined)
		return usageFailure('exactly one plan file required');
	return positionals[0];
}
export function isMain(url: string): boolean {
	const script = process.argv[1];
	return script !== undefined && pathToFileURL(script).href === url;
}
export function reportCliFailure(error: unknown): void {
	process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
	process.exitCode = error instanceof CliUsageError ? 2 : 1;
}
