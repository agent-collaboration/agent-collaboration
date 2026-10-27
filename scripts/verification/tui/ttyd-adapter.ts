/**
 * Restrict VHS's ttyd to one preconfigured guarded fixture.
 *
 * VHS finds `ttyd` first on PATH in the capture's own shim directory. That
 * fixed shell shim, written by the Rust capture driver, sends its stderr to the
 * precreated owned ttyd.stderr and execs this file with the absolute Node
 * binary. The adapter records the invocation, accepts only the exact argument
 * list VHS uses for a capture, writes ttyd.pid and execve()s the declared real
 * ttyd with this Node binary, fixture-child.ts and the launch record path as
 * its command, so the real ttyd keeps this PID and these standard descriptors
 * and its command line names this capture's output.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { z } from 'zod';

/** The arguments VHS passes after --port=<n> for every capture. */
export const EXPECTED_VHS_TAIL = [
	'--interface',
	'127.0.0.1',
	'-t',
	'rendererType=canvas',
	'-t',
	'disableResizeOverlay=true',
	'-t',
	'enableSixel=true',
	'-t',
	'customGlyphs=true',
	'--once',
	'--writable',
	'bash',
	'--noprofile',
	'--norc',
	'--login',
	'+o',
	'history',
] as const;

/** The port argument plus the ttyd options kept; VHS's bash command is replaced. */
const KEPT_TTYD_ARGUMENTS = 13;

const adapterCommandSchema = z.object({
	ttyd: z.string().min(1),
	output: z.string().min(1),
	dynamic_library_path: z.string().min(1),
});
export type AdapterCommand = z.infer<typeof adapterCommandSchema>;

/** The exec this adapter performs for one VHS invocation. */
export type TtydInvocation =
	| { readonly kind: 'version'; readonly arguments: readonly string[] }
	| { readonly kind: 'capture'; readonly port: number; readonly arguments: readonly string[] };

export class TtydInvocationRejected extends Error {
	public override readonly name = 'TtydInvocationRejected';
}

export function parseAdapterCommand(raw: unknown): AdapterCommand {
	return adapterCommandSchema.parse(raw);
}

/**
 * Classify VHS's ttyd arguments and build the real ttyd's argv (with argv0).
 * Anything other than `--version` or the exact capture form is rejected.
 */
export function planTtydInvocation(props: {
	readonly ttyd: string;
	readonly vhsArguments: readonly string[];
	readonly nodeExecutable: string;
	readonly fixtureChild: string;
	readonly fixtureCommand: string;
}): TtydInvocation {
	const { ttyd, vhsArguments, nodeExecutable, fixtureChild, fixtureCommand } = props;
	if (vhsArguments.length === 1 && vhsArguments[0] === '--version') {
		return { kind: 'version', arguments: [ttyd, '--version'] };
	}
	const [portArgument, ...tail] = vhsArguments;
	if (
		portArgument === undefined ||
		!portArgument.startsWith('--port=') ||
		tail.length !== EXPECTED_VHS_TAIL.length ||
		tail.some((argument, index) => argument !== EXPECTED_VHS_TAIL[index])
	) {
		throw new TtydInvocationRejected('Rejected unexpected VHS ttyd invocation');
	}
	const portText = portArgument.slice('--port='.length);
	const port = /^\d{1,5}$/u.test(portText) ? Number(portText) : Number.NaN;
	if (!Number.isInteger(port) || port <= 0 || port >= 65536) {
		throw new TtydInvocationRejected('Rejected invalid capture port');
	}
	return {
		kind: 'capture',
		port,
		arguments: [
			ttyd,
			...vhsArguments.slice(0, KEPT_TTYD_ARGUMENTS),
			nodeExecutable,
			fixtureChild,
			fixtureCommand,
		],
	};
}

function main(): void {
	const commandPath = process.env['QUOTA_STORYBOARD_FIXTURE_COMMAND'];
	if (commandPath === undefined) {
		throw new TtydInvocationRejected('QUOTA_STORYBOARD_FIXTURE_COMMAND is required');
	}
	if (typeof process.execve !== 'function') {
		throw new TtydInvocationRejected('process.execve is unavailable on this platform');
	}
	const command = parseAdapterCommand(JSON.parse(readFileSync(commandPath, 'utf8')));
	const environment: Record<string, string> = {};
	for (const [name, value] of Object.entries(process.env)) {
		if (value !== undefined) environment[name] = value;
	}
	environment['DYLD_LIBRARY_PATH'] = command.dynamic_library_path;
	const vhsArguments = process.argv.slice(2);
	const fixtureChild = join(dirname(fileURLToPath(import.meta.url)), 'fixture-child.ts');
	if (!(vhsArguments.length === 1 && vhsArguments[0] === '--version')) {
		writeFileSync(join(command.output, 'ttyd-invocation.json'), JSON.stringify(vhsArguments));
	}
	const invocation = planTtydInvocation({
		ttyd: command.ttyd,
		vhsArguments,
		nodeExecutable: process.execPath,
		fixtureChild,
		fixtureCommand: commandPath,
	});
	if (invocation.kind === 'capture') {
		writeFileSync(join(command.output, 'ttyd.pid'), String(process.pid));
	}
	process.execve(command.ttyd, [...invocation.arguments], environment);
}

if (import.meta.main) {
	main();
}
