/**
 * Launch only the declared fixture on the inherited real PTY; preserve teardown.
 *
 * The Rust-owned PTY driver starts this file with
 * QUOTA_STORYBOARD_FIXTURE_COMMAND naming its launch record and the same path
 * as the only argument, so each owned wrapper's command line names its own
 * capture output. The fixture child
 * inherits fds 0/1/2, the actual PTY. This process never reads stdin and never
 * opens a TTY handle on fd 1, so the terminal it shares with the child stays
 * untouched; each viewport read is a bounded `/bin/stty size` query of the
 * kernel window size, with the inherited fd 1 as its stdin.
 *
 * Effects, all inside the launch record's owned output directory:
 * quota-wrapper.pid, quota-wrapper.identity (public and component surfaces),
 * quota-child.pid, quota-child.viewport ("columns rows"), quota-child.exit,
 * component-process.exit and component-receipt.error (component surface). The
 * last line written to the terminal is QUOTA_STORYBOARD_CHILD_EXIT=<code>.
 */
import { spawn, spawnSync } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
	accessSync,
	constants,
	existsSync,
	lstatSync,
	readFileSync,
	realpathSync,
	statSync,
	writeFileSync,
	writeSync,
} from 'node:fs';
import { constants as osConstants } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';

import { z } from 'zod';

import {
	SESSIONS_RECEIPT_DIRECTORY,
	sessionsReceiptEnvironment,
} from './sessions-receipt-directory.ts';

export const COMPONENT_TEST =
	'quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture';

export const COMPONENT_SCENARIOS = {
	normal: 'Normal',
	'inspection-pending': 'InspectionPending',
	'commit-pending': 'CommitPending',
	'unknown-result': 'UnknownResult',
	'save-failed': 'SaveFailed',
	'disabled-yes': 'DisabledYes',
	'revalidation-pending': 'RevalidationPending',
	'refused-result': 'RefusedResult',
	'refresh-pending': 'RefreshPending',
	'save-pending': 'SavePending',
	'visual-hard-cases': 'VisualHardCases',
} as const satisfies Readonly<Record<string, string>>;

type ComponentScenario = keyof typeof COMPONENT_SCENARIOS;

const PENDING_SCENARIOS: ReadonlySet<ComponentScenario> = new Set<ComponentScenario>([
	'inspection-pending',
	'commit-pending',
	'revalidation-pending',
	'refresh-pending',
	'save-pending',
]);

const PUBLIC_SESSIONS_FIELDS: ReadonlySet<string> = new Set([
	'surface',
	'binary',
	'binary_sha256',
	'approved_cwd',
	'debug_router_root',
	'expected_service_id',
	'expected_service_epoch',
	'output',
	'temporary_directory',
	'dynamic_library_path',
]);

const capturedTuiSurfaceSchema = z.enum([
	'quota',
	'sessions',
	'quota-component',
	'public-sessions',
]);
type CapturedTuiSurface = z.infer<typeof capturedTuiSurfaceSchema>;

/** The launch record: every field is named text, as the Rust driver writes it. */
const fixtureCommandSchema = z.record(z.string(), z.string());
export type FixtureCommand = Readonly<z.infer<typeof fixtureCommandSchema>>;

const hostManifestSchema = z.record(z.string(), z.unknown());

/** A launch record or completion receipt the declared contract rejects. */
export class FixtureLaunchRejected extends Error {
	public override readonly name = 'FixtureLaunchRejected';
}

export interface ChildLaunch {
	readonly executable: string;
	readonly arguments: readonly string[];
	readonly environment: Readonly<Record<string, string>>;
	readonly cwd: string | undefined;
}

export function requiredText(configuration: FixtureCommand, key: string): string {
	const value = configuration[key];
	if (value === undefined || value === '') {
		throw new FixtureLaunchRejected(`missing fixture text: ${key}`);
	}
	return value;
}

function isSymlink(path: string): boolean {
	try {
		return lstatSync(path).isSymbolicLink();
	} catch {
		return false;
	}
}

/** A path the child will create: absent, not a link, directly inside output. */
function ownedFuturePath(configuration: FixtureCommand, key: string): string {
	const output = requiredText(configuration, 'output');
	const path = requiredText(configuration, key);
	const outputIsDirectory = existsSync(output) && statSync(output).isDirectory();
	if (
		!isAbsolute(output) ||
		!outputIsDirectory ||
		isSymlink(output) ||
		!isAbsolute(path) ||
		!existsSync(dirname(path)) ||
		realpathSync(dirname(path)) !== realpathSync(output) ||
		existsSync(path) ||
		isSymlink(path)
	) {
		throw new FixtureLaunchRejected(`${key} must be absent inside the owned output directory`);
	}
	return path;
}

function isComponentScenario(value: string): value is ComponentScenario {
	return Object.hasOwn(COMPONENT_SCENARIOS, value);
}

function fileSha256(path: string): string {
	return createHash('sha256').update(readFileSync(path)).digest('hex');
}

/** The canonical lowercase hyphenated form of a UUID the Host manifest carries. */
function canonicalUuid(value: string): string {
	const hex = value
		.replaceAll('urn:', '')
		.replaceAll('uuid:', '')
		.replaceAll(/^[{}]+|[{}]+$/gu, '')
		.replaceAll('-', '');
	if (!/^[0-9a-fA-F]{32}$/u.test(hex)) {
		throw new FixtureLaunchRejected('public Sessions Host service/epoch mismatch');
	}
	const lower = hex.toLowerCase();
	return `${lower.slice(0, 8)}-${lower.slice(8, 12)}-${lower.slice(12, 16)}-${lower.slice(16, 20)}-${lower.slice(20)}`;
}

function realpathStable(path: string): boolean {
	try {
		return realpathSync(path) === path;
	} catch {
		return false;
	}
}

function publicSessionsLaunch(configuration: FixtureCommand, binary: string): ChildLaunch {
	const fields = Object.keys(configuration);
	if (
		fields.length !== PUBLIC_SESSIONS_FIELDS.size ||
		fields.some((field) => !PUBLIC_SESSIONS_FIELDS.has(field))
	) {
		throw new FixtureLaunchRejected('public Sessions requires exact bound configuration fields');
	}
	let executableMatches = false;
	if (isAbsolute(binary) && realpathStable(binary) && statSync(binary).isFile()) {
		try {
			accessSync(binary, constants.X_OK);
			executableMatches = fileSha256(binary) === requiredText(configuration, 'binary_sha256');
		} catch {
			executableMatches = false;
		}
	}
	if (!executableMatches) {
		throw new FixtureLaunchRejected('public Sessions binary binding changed');
	}
	for (const key of ['approved_cwd', 'debug_router_root']) {
		const directory = requiredText(configuration, key);
		if (
			!isAbsolute(directory) ||
			!realpathStable(directory) ||
			!statSync(directory).isDirectory()
		) {
			throw new FixtureLaunchRejected('public Sessions directory binding changed');
		}
	}
	const root = requiredText(configuration, 'debug_router_root');
	const manifestPath = join(root, 'agent-communication/service.json');
	if (
		isSymlink(manifestPath) ||
		!existsSync(manifestPath) ||
		dirname(realpathSync(manifestPath)) !== join(root, 'agent-communication')
	) {
		throw new FixtureLaunchRejected('public Sessions Host manifest location changed');
	}
	const parsed = hostManifestSchema.safeParse(JSON.parse(readFileSync(manifestPath, 'utf8')));
	if (!parsed.success) {
		throw new FixtureLaunchRejected('public Sessions Host service/epoch mismatch');
	}
	for (const [field, expected] of [
		['serviceId', 'expected_service_id'],
		['serviceEpoch', 'expected_service_epoch'],
	] as const) {
		const value = parsed.data[field];
		if (
			typeof value !== 'string' ||
			canonicalUuid(value) !== requiredText(configuration, expected)
		) {
			throw new FixtureLaunchRejected('public Sessions Host service/epoch mismatch');
		}
	}
	const environment: Record<string, string> = {};
	for (const [name, value] of Object.entries(process.env)) {
		if (value !== undefined) environment[name] = value;
	}
	environment['CODEX_ROUTER_DEBUG_ROUTER_ROOT'] = root;
	environment['TERM'] = 'xterm-256color';
	environment['LANG'] = 'C.UTF-8';
	return {
		executable: binary,
		arguments: [],
		environment,
		cwd: requiredText(configuration, 'approved_cwd'),
	};
}

/** The exact child argv, environment and working directory for one surface. */
export function childLaunchConfiguration(
	configuration: FixtureCommand,
	launchEnvironment: NodeJS.ProcessEnv = {},
): ChildLaunch {
	const surfaceResult = capturedTuiSurfaceSchema.safeParse(requiredText(configuration, 'surface'));
	if (!surfaceResult.success) {
		throw new FixtureLaunchRejected('unknown captured TUI surface');
	}
	const surface: CapturedTuiSurface = surfaceResult.data;
	const binary = requiredText(configuration, 'binary');
	if (surface === 'public-sessions') {
		return publicSessionsLaunch(configuration, binary);
	}
	const temporary = requiredText(configuration, 'temporary_directory');
	const environment: Record<string, string> = {
		HOME: temporary,
		TERM: 'xterm-256color',
		LANG: 'C.UTF-8',
		TMPDIR: temporary,
	};
	switch (surface) {
		case 'quota-component': {
			const scenario = requiredText(configuration, 'component_fixture_scenario');
			if (!isComponentScenario(scenario)) {
				throw new FixtureLaunchRejected('unknown component fixture scenario');
			}
			const receipt = ownedFuturePath(configuration, 'component_fixture_receipt');
			environment['QUOTA_COMPONENT_FIXTURE'] = scenario;
			environment['QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE'] = receipt;
			if (PENDING_SCENARIOS.has(scenario)) {
				const release = ownedFuturePath(configuration, 'component_fixture_release');
				if (release === receipt) {
					throw new FixtureLaunchRejected('fixture release and completion paths must differ');
				}
				environment['QUOTA_COMPONENT_FIXTURE_RELEASE_FILE'] = release;
			}
			return {
				executable: binary,
				arguments: [COMPONENT_TEST, '--exact', '--ignored', '--nocapture', '--test-threads=1'],
				environment,
				cwd: undefined,
			};
		}
		case 'quota':
			return {
				executable: binary,
				arguments: [
					'--router-root',
					requiredText(configuration, 'root'),
					'--fixture-capability',
					requiredText(configuration, 'capability'),
					'--provider-listener',
					requiredText(configuration, 'provider'),
				],
				environment,
				cwd: undefined,
			};
		case 'sessions':
			Object.assign(
				environment,
				sessionsReceiptEnvironment(
					launchEnvironment[SESSIONS_RECEIPT_DIRECTORY],
					requiredText(configuration, 'output'),
				),
			);
			return {
				executable: binary,
				arguments: [
					'--sessions-picker',
					'--provider-filter-storyboard',
					'--fixture-capability',
					requiredText(configuration, 'capability'),
				],
				environment,
				cwd: undefined,
			};
		default:
			return assertNever(surface);
	}
}

function assertNever(value: never): never {
	throw new FixtureLaunchRejected(`unhandled captured TUI surface: ${JSON.stringify(value)}`);
}

/** The component fixture's exact completion receipt, and its owned release when pending. */
export function verifyComponentCompletion(configuration: FixtureCommand): void {
	const scenario = requiredText(configuration, 'component_fixture_scenario');
	if (!isComponentScenario(scenario)) {
		throw new FixtureLaunchRejected('unknown component fixture scenario');
	}
	const receipt = requiredText(configuration, 'component_fixture_receipt');
	if (!existsSync(receipt) || isSymlink(receipt) || !statSync(receipt).isFile()) {
		throw new FixtureLaunchRejected('component fixture completion receipt missing');
	}
	const released = PENDING_SCENARIOS.has(scenario);
	if (released) {
		const release = requiredText(configuration, 'component_fixture_release');
		if (
			isSymlink(release) ||
			!existsSync(release) ||
			readFileSync(release, 'latin1') !== 'settle\n'
		) {
			throw new FixtureLaunchRejected('component fixture lacks exact owned release');
		}
	}
	const expected =
		`QUOTA_COMPONENT_FIXTURE_FINISHED scenario=${COMPONENT_SCENARIOS[scenario]} ` +
		`released=${String(released)} external_requests=0 persisted_writes=0\n`;
	if (!readFileSync(receipt).equals(Buffer.from(expected, 'utf8'))) {
		throw new FixtureLaunchRejected('component fixture completion receipt mismatch');
	}
}

/**
 * The launch record path: QUOTA_STORYBOARD_FIXTURE_COMMAND, repeated as this
 * process's only argument so its command line names the owned capture.
 */
export function fixtureCommandPath(
	commandArguments: readonly string[],
	environmentPath: string | undefined,
): string {
	if (environmentPath === undefined) {
		throw new FixtureLaunchRejected('QUOTA_STORYBOARD_FIXTURE_COMMAND is required');
	}
	const [path, ...extra] = commandArguments;
	if (path === undefined || extra.length > 0 || !isAbsolute(path)) {
		throw new FixtureLaunchRejected('exactly one absolute fixture command path is required');
	}
	if (environmentPath !== path) {
		throw new FixtureLaunchRejected(
			'fixture command path must match QUOTA_STORYBOARD_FIXTURE_COMMAND',
		);
	}
	return path;
}

/** The launch record at the fixture command path, parsed once. */
export function readFixtureCommand(path: string): FixtureCommand {
	const raw: unknown = JSON.parse(readFileSync(path, 'utf8'));
	if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
		throw new FixtureLaunchRejected('fixture configuration must be an object');
	}
	const parsed = fixtureCommandSchema.safeParse(raw);
	if (!parsed.success) {
		throw new FixtureLaunchRejected('fixture configuration requires named text fields');
	}
	return parsed.data;
}

/** The kernel window size of the inherited terminal, as "columns rows". */
function readTerminalViewport(): string {
	const result = spawnSync('/bin/stty', ['size'], {
		stdio: [1, 'pipe', 'pipe'],
		encoding: 'utf8',
		timeout: 2000,
	});
	const match = result.status === 0 ? /^(\d+) (\d+)\n?$/u.exec(result.stdout) : null;
	if (match === null) {
		throw new FixtureLaunchRejected(`terminal viewport unavailable: ${result.stderr.trim()}`);
	}
	const [, rows, columns] = match;
	return `${columns} ${rows}`;
}

/**
 * The wrapper's `quota-wrapper.identity` receipt: the raw `uid=,lstart=,command=` line, which
 * public-grid-capture.ts and the Rust TerminalDriver compare byte for byte before any signal.
 * Ownership is decided separately by the real UID (`ruid`), the same kind as process.getuid().
 */
function processIdentityLine(pid: number): string {
	const owner = spawnSync('/bin/ps', ['-p', String(pid), '-o', 'ruid='], {
		encoding: 'utf8',
		timeout: 3000,
	});
	if (owner.status !== 0 || owner.stdout.trim() !== String(process.getuid?.())) {
		throw new FixtureLaunchRejected('public wrapper UID observation failed');
	}
	const identity = spawnSync('/bin/ps', ['-p', String(pid), '-o', 'uid=,lstart=,command='], {
		encoding: 'utf8',
		timeout: 3000,
	});
	if (identity.status !== 0) {
		throw new FixtureLaunchRejected('public wrapper UID observation failed');
	}
	return identity.stdout;
}

/** The child's exit as the original wrapper reported it: a negative signal number when signalled. */
function childExitCode(code: number | null, signal: NodeJS.Signals | null): number {
	if (code !== null) return code;
	if (signal !== null) return -osConstants.signals[signal];
	return 1;
}

function waitForChild(child: ChildProcess): Promise<number> {
	return new Promise((resolveExit, rejectExit) => {
		child.once('error', rejectExit);
		child.once('exit', (code, signal) => {
			resolveExit(childExitCode(code, signal));
		});
	});
}

async function main(): Promise<number> {
	const configuration = readFixtureCommand(
		fixtureCommandPath(process.argv.slice(2), process.env['QUOTA_STORYBOARD_FIXTURE_COMMAND']),
	);
	const launch = childLaunchConfiguration(configuration, process.env);
	const output = requiredText(configuration, 'output');
	const surface = requiredText(configuration, 'surface');
	writeFileSync(join(output, 'quota-wrapper.pid'), String(process.pid));
	if (surface === 'public-sessions' || surface === 'quota-component') {
		writeFileSync(join(output, 'quota-wrapper.identity'), processIdentityLine(process.pid));
	}
	const child = spawn(launch.executable, [...launch.arguments], {
		env: launch.environment,
		cwd: launch.cwd,
		stdio: 'inherit',
	});
	const exited = waitForChild(child);
	if (child.pid === undefined) {
		return await exited;
	}
	writeFileSync(join(output, 'quota-child.pid'), String(child.pid));
	const terminateOwnedChild = (): void => {
		if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
	};
	const recordViewport = (): void => {
		writeFileSync(join(output, 'quota-child.viewport'), readTerminalViewport());
	};
	process.on('SIGTERM', terminateOwnedChild);
	process.on('SIGINT', terminateOwnedChild);
	process.on('SIGWINCH', recordViewport);
	recordViewport();
	let exitCode = await exited;
	recordViewport();
	if (surface === 'quota-component') {
		writeFileSync(join(output, 'component-process.exit'), String(exitCode));
		if (exitCode === 0) {
			try {
				verifyComponentCompletion(configuration);
			} catch (error: unknown) {
				writeFileSync(
					join(output, 'component-receipt.error'),
					error instanceof Error ? error.message : String(error),
				);
				exitCode = 2;
			}
		}
	}
	writeFileSync(join(output, 'quota-child.exit'), String(exitCode));
	// The component marker stays on the terminal current line after PTY scrollback.
	writeSync(
		1,
		surface === 'quota-component'
			? `\nQUOTA_STORYBOARD_CHILD_EXIT=${String(exitCode)}`
			: `QUOTA_STORYBOARD_CHILD_EXIT=${String(exitCode)}\n`,
	);
	return exitCode;
}

if (import.meta.main) {
	process.exit(await main());
}
