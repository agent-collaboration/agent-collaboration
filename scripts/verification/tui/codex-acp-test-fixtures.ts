/** A declared Codex ACP target over controlled stand-ins: Node --version stub, Host manifest,
 * live endpoint socket, trusted cwd and correlated observer plan. Never installed Codex proof. */
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Server } from 'node:net';
import { join } from 'node:path';

import { afterEach } from 'vitest';

import { codexAcpTargetSchema } from './codex-acp-target.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';
import { controlledExecutable } from './controlled-child-fixtures.ts';
import { captureArtifact } from './source-binding.ts';

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

export async function targetFixture(alternateScreen = false): Promise<TargetFixture> {
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
