import { symlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { homedir } from 'node:os';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { captureArtifact } from '../evidence/source-binding.ts';
import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import {
	codexAcpTargetSchema,
	codexLaunchRecord,
	qualifyCodexAcpTarget,
	routerSession,
} from './codex-acp-target.ts';
import { ALIAS, RAW_SESSION, REFERENCE_MODEL, targetFixture } from './codex-acp-test-fixtures.ts';

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
			'-c',
			'tui.status_line=["model-with-reasoning"]',
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
	controlledExecutable(root, 'codex', "process.stdout.write('codex-cli 0.166.0\\n');");
	expect(() => qualifyCodexAcpTarget(target)).toThrow('artifact changed');
	const other = await targetFixture();
	controlledExecutable(other.root, 'codex', "process.stdout.write('codex-cli 0.165.0\\n');");
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

test.each([
	['session_id', ALIAS],
	['native_thread_id', '019a0000-0000-7000-8000-0000000000ab'],
	['endpoint_id', 'claude-local'],
	['codex_sha256', '0'.repeat(64)],
] as const)('the observer must match the client and Session: %s', async (field, value) => {
	const { root, target, observerPlan } = await targetFixture();
	writeFileSync(
		join(root, 'observer-plan.json'),
		JSON.stringify({ ...observerPlan, [field]: value }),
	);
	expect(() => qualifyCodexAcpTarget(target), field).toThrow(
		'supporting observer plan addresses a different client or Session',
	);
});

test('the launch cwd may differ from the Session cwd the observer declares', async () => {
	const { root, target, observerPlan } = await targetFixture();
	// The provider Session runs in its private fixture workspace; Codex launches in the trusted repo.
	writeFileSync(
		join(root, 'observer-plan.json'),
		JSON.stringify({ ...observerPlan, cwd: join(root, 'provider workspace') }),
	);
	const launch = qualifyCodexAcpTarget(target);
	expect(launch.cwd).toBe(target.cwd);
	expect(
		launch.arguments.slice(launch.arguments.indexOf('--cd'), launch.arguments.indexOf('--cd') + 2),
	).toEqual(['--cd', target.cwd]);
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

test('the real-Codex reference endpoint qualifies without an observer plan and launches 0.166.0', async () => {
	const { root, target } = await targetFixture(false, 'codex-local');
	expect(target.observer_plan).toBeNull();
	const launch = qualifyCodexAcpTarget(target);
	// N2 relay: the Host publishes the nativeCodex channel as codex-native.sock.
	expect(launch.arguments).toContain(`unix://${root}/service/codex-native.sock`);
	expect(launch.arguments.join(' ')).not.toContain('router-sessions');
	// It starts its own thread with the declared model: real Codex cannot resume a turn-less thread
	// (Correction 29), and the owner's default model must never apply.
	expect(launch.arguments).not.toContain('resume');
	expect(launch.arguments.slice(-4)).toEqual([
		'-m',
		'gpt-6-luna',
		'-c',
		'model_reasoning_effort="low"',
	]);
});

test('a Router target still resumes its declared Session, with no model flags', async () => {
	const { target } = await targetFixture();
	const launch = qualifyCodexAcpTarget(target);
	expect(launch.arguments.slice(-2)).toEqual(['resume', ALIAS]);
	expect(launch.arguments).not.toContain('-m');
	expect(launch.arguments.join(' ')).not.toContain('model_reasoning_effort');
});

test('the reference has no Router Session and a Router target has no reference model', async () => {
	const reference = await targetFixture(false, 'codex-local');
	expect(() => routerSession(reference.target)).toThrow('has no Router Session');
	const session = 'the real-Codex reference starts its own thread with a declared model';
	expect(() =>
		codexAcpTargetSchema.parse({ ...reference.target, native_thread_id: ALIAS }),
	).toThrow(session);
	expect(() => codexAcpTargetSchema.parse({ ...reference.target, reference_model: null })).toThrow(
		session,
	);
	const router = await targetFixture();
	expect(() =>
		codexAcpTargetSchema.parse({ ...router.target, reference_model: REFERENCE_MODEL }),
	).toThrow(session);
	expect(() => codexAcpTargetSchema.parse({ ...router.target, native_thread_id: null })).toThrow(
		session,
	);
});

test('an observer plan is required for every endpoint but codex-local, and refused there', async () => {
	const claude = await targetFixture();
	expect(() => codexAcpTargetSchema.parse({ ...claude.target, observer_plan: null })).toThrow(
		'only a codex-local target has no observer plan',
	);
	const reference = await targetFixture(false, 'codex-local');
	expect(() =>
		codexAcpTargetSchema.parse({
			...reference.target,
			observer_plan: join(reference.root, 'observer-plan.json'),
		}),
	).toThrow('only a codex-local target has no observer plan');
});

test('the installed 0.165.0 Codex is refused now that the target pins 0.166.0', async () => {
	const { root, target } = await targetFixture();
	expect(() => codexAcpTargetSchema.parse({ ...target, codex_version: '0.165.0' })).toThrow();
	const stale = await targetFixture();
	controlledExecutable(
		stale.root,
		'codex',
		"if (process.argv[2] === '--version') process.stdout.write('codex-cli 0.165.0\\n');",
	);
	expect(() =>
		qualifyCodexAcpTarget({ ...stale.target, codex: captureArtifact(join(stale.root, 'codex')) }),
	).toThrow('installed Codex version differs');
	expect(root).toBeTruthy();
});

test('the footer is the declared status line, and a wire tee redirects only the remote socket', async () => {
	const { root, target } = await targetFixture();
	const teeSocket = join(root, 'tee.sock');
	const declared = codexAcpTargetSchema.parse({
		...target,
		status_line: ['model-with-reasoning', 'context-window-size'],
		wire_tee: { listen_socket: teeSocket, recording: join(root, 'run/wire.jsonl') },
	});
	const launch = codexLaunchRecord(declared);
	expect(launch.arguments).toContain(`unix://${teeSocket}`);
	expect(launch.arguments).not.toContain(
		`unix://${root}/service/router-sessions/cursor-local.sock`,
	);
	expect(launch.arguments).toContain(
		'tui.status_line=["model-with-reasoning","context-window-size"]',
	);
	// The default launch is unchanged: the endpoint socket and the model alone.
	expect(codexLaunchRecord(target).arguments).toContain('tui.status_line=["model-with-reasoning"]');
	expect(qualifyCodexAcpTarget(declared)).toEqual(launch);
});

test('a tee socket that already exists is refused before the run, and one that is missing at launch', async () => {
	const { root, target } = await targetFixture();
	const teeSocket = join(root, 'tee.sock');
	const declared = codexAcpTargetSchema.parse({
		...target,
		wire_tee: { listen_socket: teeSocket, recording: join(root, 'run/wire.jsonl') },
	});
	// At launch the wrapper requires the run's tee to be listening; it is not.
	expect(() => qualifyCodexAcpTarget(declared, 'listening')).toThrow('ENOENT');
	writeFileSync(teeSocket, 'in the way');
	expect(() => qualifyCodexAcpTarget(declared)).toThrow('tee listen socket already exists');
	expect(() => qualifyCodexAcpTarget(declared, 'listening')).toThrow('tee socket missing');
});

test('a live tee socket qualifies at launch, and bad tee declarations fail the schema', async () => {
	const { root, target } = await targetFixture();
	const teeSocket = join(root, 'tee.sock');
	const declared = codexAcpTargetSchema.parse({
		...target,
		wire_tee: { listen_socket: teeSocket, recording: join(root, 'run/wire.jsonl') },
	});
	const live = createServer();
	await new Promise<void>((resolve) => {
		live.listen(teeSocket, resolve);
	});
	try {
		expect(qualifyCodexAcpTarget(declared, 'listening').cwd).toBe(target.cwd);
	} finally {
		await new Promise<void>((resolve) => {
			live.close(() => resolve());
		});
	}
	for (const wire_tee of [
		{ listen_socket: 'tee.sock', recording: join(root, 'wire.jsonl') },
		{ listen_socket: teeSocket, recording: 'wire.jsonl' },
		{ listen_socket: teeSocket, recording: join(root, 'wire.txt') },
		{ listen_socket: join(root, 'x'.repeat(120)), recording: join(root, 'wire.jsonl') },
	])
		expect(() => codexAcpTargetSchema.parse({ ...target, wire_tee })).toThrow();
});

test('the reference window B resumes window A thread; a Router target cannot carry one (F2)', async () => {
	const reference = await targetFixture(false, 'codex-local');
	const windowB = codexAcpTargetSchema.parse({ ...reference.target, attach_thread_id: ALIAS });
	const launch = codexLaunchRecord(windowB);
	expect(launch.arguments.slice(-2)).toEqual(['resume', ALIAS]);
	expect(launch.arguments).not.toContain('-m');
	const router = await targetFixture();
	expect(() => codexAcpTargetSchema.parse({ ...router.target, attach_thread_id: ALIAS })).toThrow(
		'the real-Codex reference starts its own thread with a declared model',
	);
});
