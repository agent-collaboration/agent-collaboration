/** Real socket/file adapter controls; the producer and native PTY remain unobserved here. */
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Server, Socket } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

import { afterEach, expect, test, vi } from 'vitest';
import { z } from 'zod';

import { shortTemporaryRoot } from '../../fixtures/short-temporary-root.ts';
import { observeHandoffLifecycle } from '../../terminal/loop-ending-handoff.ts';
import { QuotaEventJournal } from './quota-event-journal.ts';
import { QuotaFixtureControl } from './quota-fixture-control.ts';
import {
	observeQuotaEvent,
	quotaGroupingHandoff,
	quotaLoginHandoff,
} from './quota-handoff-bindings.ts';

vi.mock('node:fs', async (importOriginal) => {
	const original = await importOriginal<typeof import('node:fs')>();
	return {
		...original,
		watch: (path: string, options: { persistent: boolean }) =>
			original.watch(path, options, () => {}),
	};
});

const requestSchema = z.strictObject({
	operation: z.literal('await_event'),
	requestId: z.number(),
	after_ordinal: z.number(),
	kind: z.string(),
});
const eventSchema = z.object({ kind: z.string() }).catchall(z.unknown());
const resources: {
	root: string;
	server: Server;
	sockets: Set<Socket>;
	control: QuotaFixtureControl;
}[] = [];
async function journalFixture(
	events: readonly unknown[],
	fenceOrdinal?: number,
): Promise<{
	readonly journal: QuotaEventJournal;
	readonly control: QuotaFixtureControl;
	readonly requests: z.infer<typeof requestSchema>[];
}> {
	const root = realpathSync(mkdtempSync(join(shortTemporaryRoot(), '2026-11-qh-')));
	const path = join(root, 'socket'),
		file = join(root, 'events');
	const records = events.map((event, index) => ({
		sequence: index + 1,
		event: eventSchema.parse(event),
	}));
	writeFileSync(file, records.map((record) => JSON.stringify(record) + '\n').join(''));
	const requests: z.infer<typeof requestSchema>[] = [],
		sockets = new Set<Socket>();
	const server = createServer((socket) => {
		sockets.add(socket);
		socket.on('close', () => sockets.delete(socket));
		createInterface({ input: socket, crlfDelay: Infinity }).on('line', (line: string) => {
			const request = requestSchema.parse(JSON.parse(line));
			requests.push(request);
			const record = records.findLast(
				(entry) => entry.sequence > request.after_ordinal && entry.event.kind === request.kind,
			);
			socket.write(
				JSON.stringify({
					kind: 'fixture-event-fence',
					requestId: request.requestId,
					result:
						record === undefined
							? { kind: 'ended-before-event' }
							: {
									kind: 'observed',
									ordinal: fenceOrdinal ?? record.sequence,
									eventKind: request.kind,
								},
				}) + '\n',
			);
		});
	});
	await new Promise<void>((resolve, reject) => {
		server.once('error', reject);
		server.listen(path, resolve);
	});
	const control = await QuotaFixtureControl.connect(path);
	resources.push({ root, server, sockets, control });
	return { control, journal: new QuotaEventJournal(file), requests };
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

const firstFrame = {
	key: 'F12',
	expected: [
		{
			event: { kind: 'key', code: { kind: 'fixed', name: 'F12' }, modifiers: [], phase: 'press' },
			surface: { kind: 'screen' },
			target: null,
			action: null,
		},
	],
} as const;
const groupingSettled = {
	kind: 'grouping-invocation-settled',
	facts: {
		invocation: 1,
		initialMount: 1,
		finalMount: 1,
		initialFacet: 'none',
		finalFacet: 'provider',
		storedValue: 'provider',
	},
};
const groupingStarted = {
	kind: 'grouping-invocation-started',
	invocation: 2,
	mount: 2,
	facet: 'provider',
	storePath: '/private/tmp/quota-owned/agent-proxy-ui.sqlite',
};
function groupingOptions(
	fixture: Awaited<ReturnType<typeof journalFixture>>,
): Parameters<typeof quotaGroupingHandoff>[0] {
	return {
		...fixture,
		afterSequence: 0,
		previousMount: 1,
		storePath: '/private/tmp/quota-owned/agent-proxy-ui.sqlite',
		ready: () => true,
		firstFrame,
	};
}

test('grouping crosses actual fence and file boundaries without child facts or notification delivery', async () => {
	const fixture = await journalFixture([groupingSettled, groupingStarted]);
	const handoff = quotaGroupingHandoff(groupingOptions(fixture)),
		signal = new AbortController().signal;
	const settled = await handoff.invocationSettled(signal);
	expect(await observeHandoffLifecycle(handoff, settled, signal)).toEqual({
		kind: 'invocation-remount',
		invocationSettled: { invocation: 1, initialMount: 1, finalMount: 1 },
		invocationStarted: {
			invocation: 2,
			mount: 2,
			storePath: '/private/tmp/quota-owned/agent-proxy-ui.sqlite',
		},
		remount: { mount: 2 },
	});
	expect(fixture.requests).toEqual([
		{
			operation: 'await_event',
			requestId: 1,
			after_ordinal: 0,
			kind: 'grouping-invocation-settled',
		},
		{
			operation: 'await_event',
			requestId: 2,
			after_ordinal: 1,
			kind: 'grouping-invocation-started',
		},
	]);
});

test.each([
	{
		name: 'unsaved facet',
		settled: { ...groupingSettled, facts: { ...groupingSettled.facts, storedValue: 'none' } },
		started: groupingStarted,
	},
	{
		name: 'changed store',
		settled: groupingSettled,
		started: { ...groupingStarted, storePath: '/private/tmp/other.sqlite' },
	},
	{ name: 'stale mount', settled: groupingSettled, started: { ...groupingStarted, mount: 1 } },
])('grouping refuses $name at the source-qualified reader', async ({ settled, started }) => {
	const fixture = await journalFixture([settled, started]);
	const handoff = quotaGroupingHandoff(groupingOptions(fixture)),
		signal = new AbortController().signal;
	await expect(
		(async (): Promise<unknown> =>
			observeHandoffLifecycle(handoff, await handoff.invocationSettled(signal), signal))(),
	).rejects.toThrow('quota grouping');
});

const childArguments = [
	'account',
	'login',
	'--provider',
	'openai',
	'--label',
	'effect-alpha',
	'--router-root',
	'/private/tmp/quota-owned',
];
const childTerminal = {
	kind: 'interactive',
	canonical: true,
	echo: true,
	signals: true,
	foreground: true,
};
function childEntered(code: number): unknown {
	return {
		kind: 'login-child-entered',
		receipt: { arguments: childArguments, exitCode: code, terminal: childTerminal },
	};
}
function parentExited(code: number): unknown {
	return {
		kind: 'login-parent-exited',
		childPid: 101,
		waitOrdinal: 1,
		status: { kind: 'exited', code, rawStatus: code << 8 },
	};
}
function loginOptions(
	fixture: Awaited<ReturnType<typeof journalFixture>>,
	expectedChildExitCode: 0 | 130 | 7,
): Parameters<typeof quotaLoginHandoff>[0] {
	return {
		...fixture,
		afterSequence: 0,
		previousMount: 1,
		expectedChildArguments: childArguments,
		expectedChildExitCode,
		ready: () => true,
		firstFrame,
	};
}

test.each([0, 130, 7] as const)(
	'login outcome %s uses actual parent wait and a later mount with notifications suppressed',
	async (code) => {
		const fixture = await journalFixture([
			childEntered(code),
			parentExited(code),
			{ kind: 'view-mounted', readback: { mount: 2, scope: 'quota-browse' } },
		]);
		const handoff = quotaLoginHandoff(loginOptions(fixture, code)),
			signal = new AbortController().signal;
		const entered = await handoff.childEntered(signal);
		expect(entered).toEqual({ arguments: childArguments, terminal: childTerminal });
		expect(await observeHandoffLifecycle(handoff, entered, signal)).toEqual({
			kind: 'child-login',
			childEntry: { arguments: childArguments, terminal: childTerminal },
			childExit: { kind: 'exited', code },
			remount: { mount: 2 },
		});
		expect(fixture.requests.map((request) => [request.after_ordinal, request.kind])).toEqual([
			[0, 'login-child-entered'],
			[1, 'login-parent-exited'],
			[2, 'view-mounted'],
		]);
	},
);

test('planned successful child exit never substitutes for a different actual parent wait', async () => {
	const fixture = await journalFixture([
		childEntered(0),
		parentExited(7),
		{ kind: 'view-mounted', readback: { mount: 2 } },
	]);
	const handoff = quotaLoginHandoff(loginOptions(fixture, 0)),
		signal = new AbortController().signal;
	await expect(
		observeHandoffLifecycle(handoff, await handoff.childEntered(signal), signal),
	).rejects.toThrow('actual parent login wait');
});

test('a fresh view does not replace a missing actual parent wait', async () => {
	const fixture = await journalFixture([
		childEntered(0),
		{ kind: 'view-mounted', readback: { mount: 2 } },
	]);
	const handoff = quotaLoginHandoff(loginOptions(fixture, 0)),
		signal = new AbortController().signal;
	await expect(
		observeHandoffLifecycle(handoff, await handoff.childEntered(signal), signal),
	).rejects.toThrow('ended before');
});

test('a correctly exited child with a stale mount still fails the lifecycle owner', async () => {
	const fixture = await journalFixture([
		childEntered(0),
		parentExited(0),
		{ kind: 'view-mounted', readback: { mount: 1 } },
	]);
	const handoff = quotaLoginHandoff(loginOptions(fixture, 0)),
		signal = new AbortController().signal;
	await expect(
		observeHandoffLifecycle(handoff, await handoff.childEntered(signal), signal),
	).rejects.toThrow('fresh quota remount');
});

test('a plausible fence cannot invent a flushed journal ordinal', async () => {
	const fixture = await journalFixture([groupingSettled], 9);
	await expect(
		quotaGroupingHandoff(groupingOptions(fixture)).invocationSettled(new AbortController().signal),
	).rejects.toThrow('independently flushed journal record');
});

test('latest matching fence may follow the first matching independently decoded record', async () => {
	const fixture = await journalFixture([
		{ kind: 'fixture-started', value: 1 },
		{ kind: 'fixture-started', value: 2 },
	]);
	const result = await observeQuotaEvent({
		...fixture,
		afterSequence: 0,
		kind: 'fixture-started',
		signal: new AbortController().signal,
		decode: (value) =>
			z.strictObject({ kind: z.literal('fixture-started'), value: z.number() }).parse(value),
	});
	expect(result).toEqual({ sequence: 1, event: { kind: 'fixture-started', value: 1 } });
});
