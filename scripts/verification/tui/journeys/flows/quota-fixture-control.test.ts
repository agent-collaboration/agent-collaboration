/** Real local socket tests qualify the setup client only, with no product/PTY. */
import { mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Server, Socket } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

import { afterEach, expect, test } from 'vitest';
import { z } from 'zod';

import { shortTemporaryRoot } from '../../fixtures/short-temporary-root.ts';
import { QuotaFixtureControl } from './quota-fixture-control.ts';

const resources: {
	root: string;
	server: Server;
	control: QuotaFixtureControl;
	sockets: Set<Socket>;
}[] = [];
const requestSchema = z
	.object({ requestId: z.number(), operation: z.string() })
	.catchall(z.unknown());
async function connectFixture(
	handler: (request: z.infer<typeof requestSchema>, socket: Socket) => void,
): Promise<QuotaFixtureControl> {
	const root = realpathSync(mkdtempSync(join(shortTemporaryRoot(), '2026-11-qc-'))),
		path = join(root, 'socket');
	const sockets = new Set<Socket>();
	const server = createServer((socket) => {
		sockets.add(socket);
		socket.on('close', () => sockets.delete(socket));
		createInterface({ input: socket, crlfDelay: Infinity }).on('line', (line: string) =>
			handler(requestSchema.parse(JSON.parse(line)), socket),
		);
	});
	await new Promise<void>((resolve, reject) => {
		server.once('error', reject);
		server.listen(path, resolve);
	});
	const control = await QuotaFixtureControl.connect(path);
	resources.push({ root, server, control, sockets });
	return control;
}
afterEach(async () => {
	await Promise.all(
		resources.splice(0).map(async (resource): Promise<void> => {
			resource.control.close();
			for (const socket of resource.sockets) socket.destroy();
			await new Promise<void>((resolve, reject) =>
				resource.server.close((error) => (error === undefined ? resolve() : reject(error))),
			);
			rmSync(resource.root, { recursive: true, force: true });
		}),
	);
});

test('one setup connection preserves strictly increasing IDs and separate acknowledgement shapes', async () => {
	const requests: unknown[] = [];
	const control = await connectFixture((request, socket) => {
		requests.push(request);
		socket.write(
			JSON.stringify(
				request.operation === 'readback'
					? { kind: 'fixture-readback', requestId: request.requestId, readback: { mount: 7 } }
					: {
							kind: 'fixture-control-ack',
							requestId: request.requestId,
							operation: request.operation,
							operationId: 42,
							readback: { mount: 7, listSelection: 'effect-account-1' },
						},
			) + '\n',
		);
	});
	expect(await control.request({ operation: 'readback' })).toEqual({
		kind: 'fixture-readback',
		requestId: 1,
		readback: { mount: 7 },
	});
	expect(
		await control.request({
			operation: 'select-browse-account',
			mount: 7,
			accountId: 'effect-account-1',
		}),
	).toMatchObject({ kind: 'fixture-control-ack', requestId: 2, operationId: 42 });
	expect(requests).toEqual([
		{ operation: 'readback', requestId: 1 },
		{ operation: 'select-browse-account', mount: 7, accountId: 'effect-account-1', requestId: 2 },
	]);
});
test('a mismatched acknowledgement makes the mutation unknown and never reconnects or replays', async () => {
	const requests: unknown[] = [];
	const control = await connectFixture((request, socket) => {
		requests.push(request);
		socket.write(
			JSON.stringify({
				kind: 'fixture-mutation',
				requestId: request.requestId + 1,
				operation: request.operation,
			}) + '\n',
		);
	});
	await expect(control.request({ operation: 'reenable-before-removal' })).rejects.toThrow(
		'outcome unknown',
	);
	await expect(control.request({ operation: 'readback' })).rejects.toThrow('outcome unknown');
	expect(requests).toHaveLength(1);
});
test('setup cannot manufacture native sequence/event/surface facts', async () => {
	const control = await connectFixture((request, socket) =>
		socket.write(
			JSON.stringify({
				kind: 'fixture-control-ack',
				requestId: request.requestId,
				operation: request.operation,
				operationId: 1,
				sequence: 99,
				event: { kind: 'key' },
				surface: { kind: 'screen' },
			}) + '\n',
		),
	);
	await expect(control.request({ operation: 'reload-view-model', mount: 1 })).rejects.toThrow(
		'claims a native input',
	);
});
test('an explicit stale-mount refusal is not confused with a successful control acknowledgement', async () => {
	const control = await connectFixture((request, socket) =>
		socket.write(
			JSON.stringify({
				kind: 'fixture-control-error',
				requestId: request.requestId,
				message: 'stale mount',
			}) + '\n',
		),
	);
	await expect(control.request({ operation: 'reload-view-model', mount: 1 })).rejects.toThrow(
		'was refused: stale mount',
	);
});
