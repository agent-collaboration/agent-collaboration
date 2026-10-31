/**
 * The actual `checkCodexAcp` preflight for a Claude route proof, on literal fixture files: the
 * bound collaboration CLI's bytes, the observer plan's Host and CLI, and the normal build
 * manifest's `agent-collaboration` entry are all checked for real. Controlled evidence only: no
 * signed driver, installed Codex, Host, provider, SDK, Chrome or real target is involved, so this
 * proves the preflight boundary, never a qualified target.
 */
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { inventoryStandIn } from '../../../verification/observations/tests/claude-route-fixture.ts';
import { checkCodexAcp, codexAcpRunPlanSchema, routeBaselineScene } from './codex-acp-run.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { codexAcpTargetSchema, qualifyCodexAcpTarget } from './codex-acp-target.ts';
import {
	ALIAS,
	EPOCH,
	RAW_SESSION,
	SERVICE,
	claudeRouteHostFixture,
} from './codex-acp-test-fixtures.ts';
import { controlledExecutable, initializedSourceRepository } from './controlled-child-fixtures.ts';
import { captureArtifact, sourceBinding } from './source-binding.ts';
import { qualifyTerminal } from './terminal-qualification.ts';

// The three Vitest stand-ins, each for a prerequisite before the route binding that it does not
// own. Everything else in `checkCodexAcp` runs unmocked.
// 1. `codesign --verify --strict` of the signed driver; every other child process call is real.
vi.mock('node:child_process', async (importOriginal) => {
	const original = await importOriginal<typeof import('node:child_process')>();
	return {
		...original,
		execFileSync: vi.fn((file: string, ...rest: unknown[]): unknown =>
			file === '/usr/bin/codesign'
				? Buffer.alloc(0)
				: Reflect.apply(original.execFileSync, undefined, [file, ...rest]),
		),
	};
});
// 2. The wterm emulator qualification.
vi.mock('./terminal-qualification.ts', () => ({
	qualifyTerminal: vi.fn(async (): Promise<void> => undefined),
}));
// 3. Target qualification (installed Codex --version, the live endpoint socket and the observer
// correlation); the target schema stays real.
vi.mock('./codex-acp-target.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./codex-acp-target.ts')>()),
	qualifyCodexAcpTarget: vi.fn(),
}));

beforeEach(() => {
	vi.clearAllMocks();
});

const sourceRepositories: string[] = [];
afterEach(() => {
	for (const repository of sourceRepositories.splice(0))
		rmSync(repository, { recursive: true, force: true });
});

const PROMPT = 'Reply with only the product of 4321 and 5678, digits only';
const COMPLETION = '24534638';
// The actual Node running this test, bound once: the preflight requires its own process binary.
const NODE = captureArtifact(process.execPath);
const LISTING = ['compiled_codex_acp_grid_observation', '--exact', '--list', '--ignored'];

/** One wrong or right binding, applied while its own fixture is built. */
interface Variant {
	readonly observerHostPid?: number;
	readonly observerCli?: string;
	readonly manifestCli?: (cli: { path: string; sha256: string }) => unknown;
	readonly changeCliBytes?: boolean;
}
interface PreflightCase {
	readonly plan: CodexAcpRunPlan;
	readonly cliPath: string;
	readonly cliCalls: () => readonly unknown[];
	readonly driverCalls: () => string[][];
	readonly codexRan: () => boolean;
}

/**
 * A fresh, immutable fixture per case: its own Host run directory and marker, source repository,
 * driver and Codex stand-ins, collaboration CLI, observer plan and normal build manifest.
 */
function preflightCase(variant: Variant = {}): PreflightCase {
	const host = claudeRouteHostFixture('');
	const root = dirname(host.runDirectory);
	const repository = initializedSourceRepository();
	sourceRepositories.push(repository);
	// The compiled-test listing runs this controlled driver stand-in; it logs every invocation.
	const driverLog = join(root, 'driver-calls.jsonl');
	const driver = controlledExecutable(
		root,
		'quota_storyboard_capture_test',
		`require('node:fs').appendFileSync(${JSON.stringify(driverLog)}, JSON.stringify(process.argv.slice(2)) + '\\n');
if (process.argv.slice(2).join(' ') === ${JSON.stringify(LISTING.join(' '))}) process.stdout.write('compiled_codex_acp_grid_observation: test\\n');
else process.exitCode = 1;`,
	);
	// Codex never runs in the preflight; this stand-in records it if it ever does.
	const codexLog = join(root, 'codex-ran');
	const codex = controlledExecutable(
		root,
		'codex',
		`require('node:fs').writeFileSync(${JSON.stringify(codexLog)}, 'ran');`,
	);
	const chrome = join(root, 'chrome-stand-in');
	writeFileSync(chrome, 'not Chrome');
	const cli = inventoryStandIn(root);
	const manifest = join(root, 'normal-build-manifest.json');
	writeFileSync(
		manifest,
		JSON.stringify({
			source_commit: 'head',
			source_tree: 'tree',
			dirty: false,
			test_guard_features: [],
			binaries: {
				'agent-collaboration':
					variant.manifestCli === undefined ? cli.binding : variant.manifestCli(cli.binding),
			},
		}),
	);
	const observerPlan = join(root, 'observer-plan.json');
	writeFileSync(
		observerPlan,
		JSON.stringify({
			host_pid: variant.observerHostPid ?? 5446,
			build_manifest: manifest,
			collaboration_cli: variant.observerCli ?? cli.binding.path,
		}),
	);
	const plan = codexAcpRunPlanSchema.parse({
		repository,
		source_head: sourceBinding(repository).head,
		driver: captureArtifact(driver),
		node: NODE,
		chrome: captureArtifact(chrome),
		keys: 'legacy',
		output: join(root, 'codex-acp-run'),
		target: codexAcpTargetSchema.parse({
			codex: captureArtifact(codex),
			codex_version: '0.165.0',
			service_directory: host.serviceDirectory,
			expected_service_id: SERVICE,
			expected_service_epoch: EPOCH,
			endpoint_id: 'claude-local',
			provider_session_id: RAW_SESSION,
			native_thread_id: ALIAS,
			cwd: root,
			alternate_screen: false,
			viewport: { columns: 120, rows: 36 },
			observer_plan: observerPlan,
		}),
		claude_route: {
			host_run_directory: host.runDirectory,
			host_pid: 5446,
			max_prefix_bytes: 65_536,
			max_segment_bytes: 4096,
			collaboration_cli: cli.binding,
		},
		journey: {
			kind: 'prompt',
			provider: 'claude',
			prompt: PROMPT,
			completion: COMPLETION,
			reply_deadline_ms: 120_000,
		},
	});
	if (variant.changeCliBytes === true) appendFileSync(cli.binding.path, '// changed\n');
	return {
		plan,
		cliPath: cli.binding.path,
		cliCalls: cli.calls,
		driverCalls: (): string[][] =>
			readFileSync(driverLog, 'utf8')
				.trim()
				.split('\n')
				.map((line): string[] => {
					const parsed: unknown = JSON.parse(line);
					if (!Array.isArray(parsed)) throw new Error('driver log row is not argv');
					return parsed.map(String);
				}),
		codexRan: (): boolean => existsSync(codexLog),
	};
}

async function refusal(plan: CodexAcpRunPlan): Promise<string> {
	try {
		await checkCodexAcp(plan);
	} catch (error: unknown) {
		if (error instanceof Error) return error.message;
		throw error;
	}
	throw new Error('the preflight accepted a wrong route binding');
}

/**
 * Every stand-in prerequisite ran and passed before the route binding, and the preflight took no
 * other action: one compiled-test listing, no Codex, no collaboration CLI read and no run output.
 */
function expectOnlyPreflight(fixture: PreflightCase): void {
	expect(vi.mocked(execFileSync)).toHaveBeenCalledWith('/usr/bin/codesign', [
		'--verify',
		'--strict',
		fixture.plan.driver.path,
	]);
	expect(vi.mocked(qualifyTerminal)).toHaveBeenCalledTimes(1);
	expect(vi.mocked(qualifyCodexAcpTarget)).toHaveBeenCalledExactlyOnceWith(fixture.plan.target);
	expect(fixture.driverCalls()).toEqual([LISTING]);
	expect(fixture.codexRan()).toBe(false);
	expect(fixture.cliCalls()).toEqual([]);
	expect(existsSync(fixture.plan.output)).toBe(false);
}

test('the declared normal build, CLI and Host pass as the ordinary Claude prompt journey', async () => {
	const fixture = preflightCase();
	expect(routeBaselineScene(fixture.plan)).toBe('prompt-composed');
	// Literal expectations (codex-acp-prompt-scenes.ts), not the builder the preflight itself returns.
	const journey = await checkCodexAcp(fixture.plan);
	expect(journey.name).toBe('codex-acp-j2-claude-prompt');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'prompt-composed',
		'prompt-replied',
	]);
	expectOnlyPreflight(fixture);
});

test('an observer plan for another Host is refused', async () => {
	const fixture = preflightCase({ observerHostPid: 5447 });
	expect(await refusal(fixture.plan)).toBe(
		'the route proof names another Host than its observer plan',
	);
	expectOnlyPreflight(fixture);
});

test('collaboration CLI bytes changed after binding are refused', async () => {
	const fixture = preflightCase({ changeCliBytes: true });
	expect(await refusal(fixture.plan)).toBe(`artifact changed: ${fixture.cliPath}`);
	expectOnlyPreflight(fixture);
});

const BUILD_MISMATCH = 'the route proof binds another collaboration CLI than the normal build';
const MISMATCHES: readonly [string, Variant][] = [
	['an observer plan running another CLI', { observerCli: '/bin/agent-collaboration' }],
	[
		'a build manifest naming another CLI path',
		{ manifestCli: (cli) => ({ ...cli, path: '/bin/agent-collaboration' }) },
	],
	[
		'a build manifest naming other CLI bytes',
		{ manifestCli: (cli) => ({ ...cli, sha256: 'f'.repeat(64) }) },
	],
	['a build manifest without the CLI entry', { manifestCli: () => undefined }],
];
for (const [name, variant] of MISMATCHES)
	test(`the normal build binding refuses ${name}`, async () => {
		const fixture = preflightCase(variant);
		expect(await refusal(fixture.plan)).toBe(BUILD_MISMATCH);
		expectOnlyPreflight(fixture);
	});
