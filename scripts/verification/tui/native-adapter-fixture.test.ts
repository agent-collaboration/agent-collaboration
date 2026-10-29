import { once } from 'node:events';
import {
	existsSync,
	mkdtempSync,
	readFileSync,
	realpathSync,
	rmSync,
	writeFileSync,
} from 'node:fs';
import { createServer } from 'node:net';
import type { Server, Socket } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { withinFixtureDeadline } from '../../../verification/fixtures/fixture-control-socket.ts';
import { nativeAdapterFixtureMain, prepareNativeAdapterFixture } from './native-adapter-fixture.ts';

const roots: string[] = [];
const servers: Server[] = [];
const sockets: Socket[] = [];
const originalExitCode = process.exitCode;
afterEach(async () => {
	vi.clearAllTimers();
	vi.useRealTimers();
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	process.exitCode = originalExitCode;
	for (const socket of sockets.splice(0)) socket.destroy();
	await Promise.all(
		servers.splice(0).map(
			(server): Promise<void> =>
				new Promise((resolve, reject) => {
					server.close((error) => (error === undefined ? resolve() : reject(error)));
				}),
		),
	);
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

async function controlFixture(root: string): Promise<{ readonly connected: Promise<Socket> }> {
	const server = createServer();
	servers.push(server);
	const connected = new Promise<Socket>((resolve, reject) => {
		server.once('connection', (socket) => {
			sockets.push(socket);
			resolve(socket);
		});
		server.once('error', reject);
	});
	const listening = once(server, 'listening');
	server.listen(join(root, 'ctl'));
	await withinFixtureDeadline(listening, 5000);
	return { connected };
}

async function expectControlByte(socket: Socket, expected: string): Promise<void> {
	const chunks: unknown = await withinFixtureDeadline(once(socket, 'data'), 5000);
	if (!Array.isArray(chunks)) throw new Error('control event arguments absent');
	const chunk: unknown = chunks[0];
	expect(Buffer.isBuffer(chunk) ? chunk.toString() : chunk).toBe(expected);
}

function pendingFixture(): { root: string; receipt: string; release: string } {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'native-adapter-release-')));
	roots.push(root);
	prepareNativeAdapterFixture(root, 'pending-valid', process.execPath);
	const receipt = join(root, 'output/fixture-finished');
	vi.stubEnv('QUOTA_COMPONENT_FIXTURE', 'commit-pending');
	vi.stubEnv('QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE', receipt);
	return { root, receipt, release: join(root, 'output/fixture-release') };
}

test('pending_fixture_observes_the_exact_release_after_control_acknowledgement', async () => {
	const { root, receipt, release } = pendingFixture();
	const { connected } = await controlFixture(root);
	const output = vi.spyOn(process.stdout, 'write').mockReturnValue(true);
	vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
	writeFileSync(release, 'wrong\n');
	const child = nativeAdapterFixtureMain(['child', root]);
	const socket = await withinFixtureDeadline(connected, 5000);
	await expectControlByte(socket, 'R');
	expect(output).toHaveBeenCalledWith('NATIVE_ADAPTER_READY\n');
	expect(existsSync(receipt)).toBe(false);
	writeFileSync(release, 'settle\n');
	const acknowledged = expectControlByte(socket, 'D');
	socket.write('X');
	await acknowledged;
	socket.write('A');
	await child;
	expect(readFileSync(receipt, 'utf8')).toBe(
		'QUOTA_COMPONENT_FIXTURE_FINISHED scenario=CommitPending released=true external_requests=0 persisted_writes=0\n',
	);
	expect(process.exitCode).toBe(0);
	expect(vi.getTimerCount()).toBe(0);
});

test('pending_fixture_validates_existing_release_at_the_explicit_notification', async () => {
	const { root, receipt, release } = pendingFixture();
	const { connected } = await controlFixture(root);
	writeFileSync(release, 'settle\n');
	vi.spyOn(process.stdout, 'write').mockReturnValue(true);
	vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
	const child = nativeAdapterFixtureMain(['child', root]);
	const socket = await withinFixtureDeadline(connected, 5000);
	await expectControlByte(socket, 'R');
	expect(existsSync(receipt)).toBe(false);
	const acknowledged = expectControlByte(socket, 'D');
	socket.write('X');
	await acknowledged;
	socket.write('A');
	await child;
	expect(readFileSync(receipt, 'utf8')).toBe(
		'QUOTA_COMPONENT_FIXTURE_FINISHED scenario=CommitPending released=true external_requests=0 persisted_writes=0\n',
	);
	expect(process.exitCode).toBe(0);
	expect(vi.getTimerCount()).toBe(0);
});
