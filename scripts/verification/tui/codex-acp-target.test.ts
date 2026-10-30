import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Server } from 'node:net';
import { homedir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import {
	codexAcpTargetSchema,
	codexLaunchRecord,
	qualifyCodexAcpTarget,
} from './codex-acp-target.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';
import { controlledExecutable } from './controlled-child-fixtures.ts';
import { captureArtifact } from './source-binding.ts';

const SERVICE = '0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89';
const EPOCH = '019a0000-0000-7000-8000-0000000000e1';
// An opaque provider Session and the Router alias the observer plan checks it against.
const RAW_SESSION = 'chat/../a b';
const ALIAS = '5756efda-4d21-59ce-aef2-2b68a02a6598';
const cleanups: (() => void)[] = [];
afterEach(() => {
	for (const cleanup of cleanups.splice(0)) cleanup();
});

interface TargetFixture {
	readonly root: string;
	readonly target: CodexAcpTarget;
	readonly observerPlan: Record<string, unknown>;
	readonly server: Server;
}

async function targetFixture(alternateScreen = false): Promise<TargetFixture> {
	// A short real root keeps the endpoint socket under the macOS Unix-socket path limit.
	const root = realpathSync(mkdtempSync('/tmp/cxt-'));
	cleanups.push(() => rmSync(root, { recursive: true, force: true }));
	// A Node stand-in for the --version probe only; never installed Codex proof.
	const codex = controlledExecutable(
		root,
		'codex',
		"if (process.argv[2] === '--version') process.stdout.write('codex-cli 0.165.0\\n');",
	);
	const service = join(root, 'service');
	mkdirSync(join(service, 'router-sessions'), { recursive: true });
	writeFileSync(
		join(service, 'service.json'),
		JSON.stringify({ serviceId: SERVICE, serviceEpoch: EPOCH }),
	);
	const server = createServer();
	await new Promise<void>((resolve) => {
		server.listen(join(service, 'router-sessions/cursor-local.sock'), resolve);
	});
	cleanups.unshift(() => server.close());
	const cwd = join(root, 'trusted worktree');
	mkdirSync(cwd);
	const binding = captureArtifact(codex);
	const observerPlan = {
		codex_cli: binding.path,
		codex_sha256: binding.sha256,
		service_directory: service,
		endpoint_id: 'cursor-local',
		session_id: RAW_SESSION,
		native_thread_id: ALIAS,
		cwd,
	};
	const observer = join(root, 'observer-plan.json');
	writeFileSync(observer, JSON.stringify(observerPlan));
	const target = codexAcpTargetSchema.parse({
		codex: binding,
		codex_version: '0.165.0',
		service_directory: service,
		expected_service_id: SERVICE,
		expected_service_epoch: EPOCH,
		endpoint_id: 'cursor-local',
		provider_session_id: RAW_SESSION,
		native_thread_id: ALIAS,
		cwd,
		alternate_screen: alternateScreen,
		viewport: { columns: 120, rows: 36 },
		observer_plan: observer,
	});
	return { root, target, observerPlan, server };
}

test('the launch resumes the native alias over the endpoint socket with hooks off', async () => {
	const { root, target } = await targetFixture();
	expect(codexLaunchRecord(target)).toEqual({
		program: join(root, 'codex'),
		arguments: [
			'--no-alt-screen',
			'--remote',
			`unix://${root}/service/router-sessions/cursor-local.sock`,
			'--cd',
			join(root, 'trusted worktree'),
			'-c',
			'features.hooks=false',
			'resume',
			ALIAS,
		],
		cwd: join(root, 'trusted worktree'),
	});
	expect(codexLaunchRecord({ ...target, alternate_screen: true }).arguments[0]).toBe('--remote');
	expect(qualifyCodexAcpTarget(target)).toEqual(codexLaunchRecord(target));
});

test('a changed or different installed Codex is refused before launch', async () => {
	const { root, target } = await targetFixture();
	controlledExecutable(root, 'codex', "process.stdout.write('codex-cli 0.165.0\\n');");
	expect(() => qualifyCodexAcpTarget(target)).toThrow('artifact changed');
	const other = await targetFixture();
	controlledExecutable(other.root, 'codex', "process.stdout.write('codex-cli 0.164.0\\n');");
	expect(() =>
		qualifyCodexAcpTarget({ ...other.target, codex: captureArtifact(join(other.root, 'codex')) }),
	).toThrow('installed Codex version differs');
});

test('the production Router, another Host and a missing or replaced socket are refused', async () => {
	const { root, target, server } = await targetFixture();
	expect(() =>
		qualifyCodexAcpTarget({ ...target, service_directory: join(homedir(), '.codex-router') }),
	).toThrow('production Router service');
	expect(() =>
		qualifyCodexAcpTarget({
			...target,
			expected_service_epoch: '019a0000-0000-7000-8000-0000000000e2',
		}),
	).toThrow('isolated Host service/epoch mismatch');
	await new Promise<void>((resolve) => {
		server.close(() => resolve());
	});
	expect(() => qualifyCodexAcpTarget(target)).toThrow();
	writeFileSync(join(root, 'service/router-sessions/cursor-local.sock'), 'not a socket');
	expect(() => qualifyCodexAcpTarget(target)).toThrow('endpoint socket missing');
});

test('a symlinked trusted cwd is refused', async () => {
	const { root, target } = await targetFixture();
	symlinkSync(join(root, 'trusted worktree'), join(root, 'linked worktree'));
	expect(() => qualifyCodexAcpTarget({ ...target, cwd: join(root, 'linked worktree') })).toThrow(
		'trusted cwd directory binding changed',
	);
});

test('the observer must read the same client, raw Session, native alias and cwd', async () => {
	const mismatches = [
		['session_id', ALIAS],
		['native_thread_id', '019a0000-0000-7000-8000-0000000000ab'],
		['endpoint_id', 'claude-local'],
		['cwd', '/elsewhere'],
		['codex_sha256', '0'.repeat(64)],
	] as const;
	const fixtures = await Promise.all(mismatches.map(async () => targetFixture()));
	for (const [index, [field, value]] of mismatches.entries()) {
		const fixture = fixtures[index];
		if (fixture === undefined) throw new Error('missing observer fixture');
		const { root, target, observerPlan } = fixture;
		writeFileSync(
			join(root, 'observer-plan.json'),
			JSON.stringify({ ...observerPlan, [field]: value }),
		);
		expect(() => qualifyCodexAcpTarget(target), field).toThrow(
			'supporting observer plan addresses a different client or Session',
		);
	}
});

test('raw provider Session IDs stay opaque while the native alias must be a UUID', async () => {
	const { target } = await targetFixture();
	expect(
		codexAcpTargetSchema.parse({ ...target, provider_session_id: 'a/b/./c' }).provider_session_id,
	).toBe('a/b/./c');
	expect(() => codexAcpTargetSchema.parse({ ...target, native_thread_id: RAW_SESSION })).toThrow();
});

test('a wrong native alias declared in both target and observer is refused before launch', async () => {
	const { root, target, observerPlan } = await targetFixture();
	const wrongAlias = '019a0000-0000-7000-8000-0000000000ab';
	writeFileSync(
		join(root, 'observer-plan.json'),
		JSON.stringify({ ...observerPlan, native_thread_id: wrongAlias }),
	);
	expect(() => qualifyCodexAcpTarget({ ...target, native_thread_id: wrongAlias })).toThrow(
		'native thread is not the Router alias of this provider Session',
	);
});
