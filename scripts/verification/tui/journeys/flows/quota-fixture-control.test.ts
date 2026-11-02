/** Real local socket tests qualify the setup client only, with no product/PTY. */
import { appendFileSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Server, Socket } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

import { afterEach, expect, test, vi } from 'vitest';
import { z } from 'zod';

import { shortTemporaryRoot } from '../../fixtures/short-temporary-root.ts';
import { QuotaEventJournal } from './quota-event-journal.ts';
import { QuotaFixtureControl } from './quota-fixture-control.ts';

// All completion must cross the actual socket, even when file notifications vanish.
vi.mock('node:fs', async (importOriginal) => {
	const original = await importOriginal<typeof import('node:fs')>();
	return {
		...original,
		watch: (path: string, options: { persistent: boolean }) =>
			original.watch(path, options, () => {}),
	};
});

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
	vi.useRealTimers();
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

const settledKind = 'grouping-invocation-settled';
function observedFence(
	requestId: number,
	ordinal = 9,
	eventKind: string = settledKind,
): Record<string, unknown> {
	return {
		kind: 'fixture-event-fence',
		requestId,
		result: { kind: 'observed', ordinal, eventKind },
	};
}

test('post-flush fence shares the existing increasing IDs without requiring an operation echo', async () => {
	const requests: unknown[] = [];
	const control = await connectFixture((request, socket) => {
		requests.push(request);
		socket.write(
			JSON.stringify(
				request.operation === 'await_event'
					? observedFence(request.requestId)
					: { kind: 'fixture-readback', requestId: request.requestId, readback: { mount: 7 } },
			) + '\n',
		);
	});
	expect(await control.request({ operation: 'readback' })).toMatchObject({ requestId: 1 });
	const completedCaller = new AbortController();
	expect(
		await control.awaitEvent({
			afterOrdinal: 7,
			kind: settledKind,
			signal: completedCaller.signal,
		}),
	).toEqual({ kind: 'observed', ordinal: 9, eventKind: settledKind });
	completedCaller.abort(new Error('completed fence no longer owns the socket'));
	expect(await control.request({ operation: 'readback' })).toMatchObject({ requestId: 3 });
	expect(requests).toEqual([
		{ operation: 'readback', requestId: 1 },
		{ operation: 'await_event', requestId: 2, after_ordinal: 7, kind: settledKind },
		{ operation: 'readback', requestId: 3 },
	]);
});

test.each([
	['old ordinal', (requestId: number) => observedFence(requestId, 7)],
	['older ordinal', (requestId: number) => observedFence(requestId, 6)],
	['wrong kind', (requestId: number) => observedFence(requestId, 9, 'grouping-invocation-started')],
	['invented kind', (requestId: number) => observedFence(requestId, 9, 'invented-kind')],
	['wrong request', (requestId: number) => observedFence(requestId + 1)],
	[
		'ended',
		(requestId: number) => ({
			kind: 'fixture-event-fence',
			requestId,
			result: { kind: 'ended-before-event' },
		}),
	],
	[
		'native receipt',
		(requestId: number) => ({
			...observedFence(requestId),
			sequence: 99,
			event: {},
			surface: { kind: 'screen' },
		}),
	],
	[
		'operation echo',
		(requestId: number) => ({ ...observedFence(requestId), operation: 'await_event' }),
	],
	[
		'setup acknowledgement',
		(requestId: number) => ({ kind: 'fixture-control-ack', requestId, operation: 'await_event' }),
	],
] as const)(
	'fence rejects %s and never continues or replays that connection',
	async (_name, reply) => {
		const requests: unknown[] = [];
		const control = await connectFixture((request, socket) => {
			requests.push(request);
			socket.write(JSON.stringify(reply(request.requestId)) + '\n');
		});
		await expect(
			control.awaitEvent({
				afterOrdinal: 7,
				kind: settledKind,
				signal: new AbortController().signal,
			}),
		).rejects.toThrow();
		await expect(control.request({ operation: 'readback' })).rejects.toThrow();
		expect(requests).toHaveLength(1);
	},
);

test('only the caller bounds a pending fence; other requests stay refused until causal completion', async () => {
	const received = Promise.withResolvers<void>();
	const control = await connectFixture(() => received.resolve());
	vi.useFakeTimers();
	const caller = new AbortController();
	const pending = control.awaitEvent({ afterOrdinal: 0, kind: settledKind, signal: caller.signal });
	const rejected = expect(pending).rejects.toThrow('caller stopped observation');
	await received.promise;
	await vi.advanceTimersByTimeAsync(10_001);
	await expect(control.request({ operation: 'readback' })).rejects.toThrow(
		'one outstanding request',
	);
	caller.abort(new Error('caller stopped observation'));
	await rejected;
	await expect(control.request({ operation: 'readback' })).rejects.toThrow(
		'caller stopped observation',
	);
});

test('unsafe ordinals and an already-aborted caller write no request', async () => {
	const requests: unknown[] = [];
	const control = await connectFixture((request, socket) => {
		requests.push(request);
		socket.write(JSON.stringify({ kind: 'fixture-readback', requestId: request.requestId }) + '\n');
	});
	await expect(
		control.awaitEvent({
			afterOrdinal: Number.MAX_SAFE_INTEGER + 1,
			kind: settledKind,
			signal: new AbortController().signal,
		}),
	).rejects.toThrow();
	const caller = new AbortController();
	caller.abort(new Error('already stopped'));
	await expect(
		control.awaitEvent({ afterOrdinal: 0, kind: settledKind, signal: caller.signal }),
	).rejects.toThrow('already stopped');
	await expect(control.request({ operation: 'readback' })).resolves.toMatchObject({ requestId: 1 });
	expect(requests).toEqual([{ operation: 'readback', requestId: 1 }]);
});

test.each(['fence', 'EOF'] as const)(
	'independent journal read uses actual %s completion with notifications suppressed',
	async (completion) => {
		let journalPath = '';
		const event = { kind: settledKind, facts: { invocation: 1, initialMount: 1, finalMount: 1 } };
		const control = await connectFixture((request, socket) => {
			appendFileSync(journalPath, JSON.stringify({ sequence: 1, event }) + '\n');
			if (completion === 'fence')
				socket.write(JSON.stringify(observedFence(request.requestId, 1)) + '\n');
			else socket.destroy();
		});
		const resource = resources.at(-1);
		if (resource === undefined) throw new Error('owned fixture resource absent');
		journalPath = join(resource.root, 'events.jsonl');
		writeFileSync(journalPath, '');
		const journal = new QuotaEventJournal(journalPath);
		const signal = new AbortController().signal;
		const observed = journal.waitFor({
			afterSequence: 0,
			kind: settledKind,
			decode: (value) =>
				z
					.object({ kind: z.literal(settledKind), facts: z.object({ invocation: z.literal(1) }) })
					.parse(value),
			signal,
			awaitCompletion: async (caller) => {
				await control.awaitEvent({ afterOrdinal: 0, kind: settledKind, signal: caller });
			},
		});
		if (completion === 'EOF') {
			await expect(observed).rejects.toThrow('connection closed');
			return;
		}
		expect(await observed).toEqual({
			sequence: 1,
			event: { kind: settledKind, facts: { invocation: 1 } },
		});
		expect(journal.read(true)).toEqual([{ sequence: 1, event }]);
	},
);
