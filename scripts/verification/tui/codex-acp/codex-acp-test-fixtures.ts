/** A declared Codex ACP target over controlled stand-ins: Node --version stub, Host manifest,
 * live endpoint socket, trusted cwd and correlated observer plan. Never installed Codex proof. */
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Server } from 'node:net';
import { join } from 'node:path';

import { afterEach } from 'vitest';

import { captureArtifact } from '../evidence/source-binding.ts';
import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { codexAcpTargetSchema } from './codex-acp-target.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';

export const SERVICE = '0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89';
export const EPOCH = '019a0000-0000-7000-8000-0000000000e1';
// An opaque provider Session and the Router alias the observer plan checks it against.
export const RAW_SESSION = 'chat/../a b';
export const ALIAS = '5756efda-4d21-59ce-aef2-2b68a02a6598';
const cleanups: (() => void)[] = [];
afterEach(() => {
	for (const cleanup of cleanups.splice(0)) cleanup();
});

export interface TargetFixture {
	readonly root: string;
	readonly target: CodexAcpTarget;
	readonly observerPlan: Record<string, unknown>;
	readonly server: Server;
}

/** The thread a recorded real-Codex reference start reports. */
export const LAUNCHED_THREAD = '019a0000-0000-7000-8000-00000000a1a1';

/**
 * A tee recording of the reference CLI's own thread/start and its reply, as Codex shapes it
 * (ThreadStartResponse: thread.id, model, modelProvider, reasoningEffort). Fixture data only.
 */
export function referenceStartRecording(
	threadId: string,
	reply: { model?: string; effort?: string | null; provider?: string } = {},
): string {
	const frames = [
		{
			seq: 0,
			connection: 0,
			direction: 'cliToServer',
			message: { ['id']: 1, method: 'initialize', params: {} },
		},
		{ seq: 1, connection: 0, direction: 'serverToCli', message: { ['id']: 1, result: {} } },
		{
			seq: 2,
			connection: 0,
			direction: 'cliToServer',
			message: { ['id']: 7, method: 'thread/start', params: { model: 'gpt-6-luna' } },
		},
		{
			seq: 3,
			connection: 0,
			direction: 'serverToCli',
			message: {
				['id']: 7,
				result: {
					thread: { ['id']: threadId },
					model: reply.model ?? 'gpt-6-luna',
					modelProvider: reply.provider ?? 'codex-router-debug',
					reasoningEffort: reply.effort === undefined ? 'low' : reply.effort,
				},
			},
		},
	];
	return frames.map((frame) => JSON.stringify(frame)).join('\n') + '\n';
}

/** The model the real-Codex reference fixture declares. */
export const REFERENCE_MODEL = {
	model: 'gpt-6-luna',
	effort: 'low',
	model_provider: 'codex-router-debug',
} as const;

/**
 * `codex-local` is the real-Codex reference endpoint: it starts its own thread with a declared
 * model (assignment Correction 29), so it has no Router Session, alias or ACP observer plan.
 */
export async function targetFixture(
	alternateScreen = false,
	endpoint: 'cursor-local' | 'codex-local' = 'cursor-local',
): Promise<TargetFixture> {
	// A short real root keeps the endpoint socket under the macOS Unix-socket path limit.
	const root = realpathSync(mkdtempSync('/tmp/cxt-'));
	cleanups.push(() => rmSync(root, { recursive: true, force: true }));
	// A Node stand-in for the --version probe only; never installed Codex proof.
	const codex = controlledExecutable(
		root,
		'codex',
		"if (process.argv[2] === '--version') process.stdout.write('codex-cli 0.166.0\\n');",
	);
	const service = join(root, 'service');
	mkdirSync(join(service, 'router-sessions'), { recursive: true });
	writeFileSync(
		join(service, 'service.json'),
		JSON.stringify({ serviceId: SERVICE, serviceEpoch: EPOCH }),
	);
	const server = createServer();
	await new Promise<void>((resolve) => {
		server.listen(
			endpoint === 'codex-local'
				? join(service, 'codex-native.sock')
				: join(service, `router-sessions/${endpoint}.sock`),
			resolve,
		);
	});
	cleanups.unshift(() => server.close());
	const cwd = join(root, 'trusted worktree');
	mkdirSync(cwd);
	const binding = captureArtifact(codex);
	const sessionId = endpoint === 'cursor-local' ? RAW_SESSION : ALIAS;
	const observerPlan = {
		codex_cli: binding.path,
		codex_sha256: binding.sha256,
		service_directory: service,
		endpoint_id: endpoint,
		session_id: sessionId,
		native_thread_id: ALIAS,
		cwd,
	};
	const observer = join(root, 'observer-plan.json');
	if (endpoint === 'cursor-local') writeFileSync(observer, JSON.stringify(observerPlan));
	const target = codexAcpTargetSchema.parse({
		codex: binding,
		codex_version: '0.166.0',
		service_directory: service,
		expected_service_id: SERVICE,
		expected_service_epoch: EPOCH,
		endpoint_id: endpoint,
		provider_session_id: endpoint === 'codex-local' ? null : sessionId,
		native_thread_id: endpoint === 'codex-local' ? null : ALIAS,
		reference_model: endpoint === 'codex-local' ? REFERENCE_MODEL : null,
		cwd,
		alternate_screen: alternateScreen,
		viewport: { columns: 120, rows: 36 },
		observer_plan: endpoint === 'cursor-local' ? observer : null,
	});
	return { root, target, observerPlan, server };
}

export interface ClaudeRouteHostFixture {
	readonly runDirectory: string;
	readonly serviceDirectory: string;
	readonly auditFile: string;
	readonly binding: {
		readonly hostRunDirectory: string;
		readonly hostPid: number;
		readonly serviceDirectory: string;
		readonly maxPrefixBytes: number;
		readonly maxSegmentBytes: number;
	};
}

/**
 * A controlled normal-Host run directory for the Claude route window: the Host's own context
 * marker (automation-debug-host.rs:32-47), the service's published Router listener and the Router
 * audit file with a declared prefix. Never a real Host, Router or audit stream.
 */
export function claudeRouteHostFixture(auditPrefix: string): ClaudeRouteHostFixture {
	const root = realpathSync(mkdtempSync('/tmp/cxr-'));
	cleanups.push(() => rmSync(root, { recursive: true, force: true }));
	const runDirectory = join(root, 'run');
	const serviceDirectory = join(root, 'router', 'agent-communication');
	mkdirSync(runDirectory, { mode: 0o700 });
	mkdirSync(serviceDirectory, { recursive: true, mode: 0o700 });
	writeFileSync(
		join(serviceDirectory, 'service.json'),
		JSON.stringify({
			serviceId: SERVICE,
			serviceEpoch: EPOCH,
			routerProxyEndpoint: '127.0.0.1:61613',
			mcp: { url: 'http://127.0.0.1:61614/mcp' },
		}),
	);
	writeFileSync(
		join(runDirectory, 'debug-host-context.json'),
		JSON.stringify({
			kind: 'debugHostPrepared',
			runDirectory,
			serviceDirectory,
			workspace: join(runDirectory, 'agent-workspace'),
			profile: 'codex-router-debug',
			model: 'gpt-6-luna',
			effort: 'low',
			port: 61613,
			hostPid: 5446,
			routerRoot: join(root, 'router'),
			claudeProvider: { executable: '/bin/claude-agent-acp', arguments: [] },
			cursorProvider: null,
			peerRegistryDirectory: null,
		}),
		{ mode: 0o600 },
	);
	const auditFile = join(runDirectory, 'router-audit.jsonl');
	writeFileSync(auditFile, auditPrefix, { mode: 0o600 });
	return {
		runDirectory,
		serviceDirectory,
		auditFile,
		binding: {
			hostRunDirectory: runDirectory,
			hostPid: 5446,
			serviceDirectory,
			maxPrefixBytes: 65_536,
			maxSegmentBytes: 4096,
		},
	};
}
