/** Controlled MCP and terminal boundary: no Host, Codex or provider process is started. */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { join } from 'node:path';

import { expect, test } from 'vitest';
import { z } from 'zod';

import { decode, encode } from '../../../../verification/exact-json.ts';
import { listenOnTestPort } from '../../../../verification/observations/mcp-control/test-support/loopback-test-port.ts';
import { createTerminalCore, gridLines, snapshotGrid } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { cliPromptReadback } from './codex-acp-mcp-readback.ts';
import { promptJourney } from './codex-acp-prompt-scenes.ts';
import { codexAcpMain, driveCodexJourney } from './codex-acp-run.ts';
import { EPOCH, RAW_SESSION, SERVICE, targetFixture } from './codex-acp-test-fixtures.ts';

const declaration = {
	expected_model: 'grok-4.7-low',
	prompt: 'Reply with only the product of 4321 and 5678, digits only',
	completion: '24534638',
	reply_deadline_ms: 120_000,
};
const targetSession = {
	endpoint: { serviceId: SERVICE, endpointId: 'cursor-local' },
	sessionId: RAW_SESSION,
};
const inputId = '019a0000-0000-7000-8000-000000000001';
const turnId = '019a0000-0000-7000-8000-000000000002';

interface CaseOptions {
	readonly models?: readonly unknown[];
	readonly foreignInspect?: boolean;
	readonly foreignObservation?: boolean;
	readonly reply?: string;
	readonly prompt?: string;
	readonly outcome?: unknown;
	readonly extraTurn?: boolean;
	readonly acceptedInput?: boolean;
	readonly changedEpoch?: boolean;
	readonly firstInspectGate?: Promise<void>;
	readonly secondInspectGate?: Promise<void>;
}

async function controlledRun(options: CaseOptions = {}): Promise<{
	readonly drive: () => Promise<string[]>;
	readonly cleanup: () => Promise<void>;
	readonly sent: string[];
	readonly reads: () => string;
	readonly result: () => unknown;
	readonly hasResult: () => boolean;
	readonly firstInspect: Promise<void>;
	readonly secondInspect: Promise<void>;
}> {
	const fixture = await targetFixture();
	const evidence = join(fixture.root, 'mcp-evidence');
	mkdirSync(evidence);
	const sent: string[] = [];
	let inspected = 0;
	let observed = 0;
	const { promise: firstInspect, resolve: markFirst } = Promise.withResolvers<void>();
	const { promise: secondInspect, resolve: markSecond } = Promise.withResolvers<void>();
	const answer = async (request: IncomingMessage, response: ServerResponse): Promise<void> => {
		const chunks: Buffer[] = [];
		for await (const chunk of request) chunks.push(Buffer.from(chunk));
		const envelope = z
			.looseObject({ method: z.string(), params: z.unknown() })
			.parse(decode(Buffer.concat(chunks).toString()));
		let value: unknown = {
			protocolVersion: '2025-11-25',
			capabilities: {},
			serverInfo: { name: 'controlled', version: '1' },
		};
		if (envelope.method === 'tools/call') {
			const call = z.object({ name: z.string(), arguments: z.unknown() }).parse(envelope.params);
			const args = z.looseObject({ target: z.unknown() }).parse(call.arguments);
			expect(args.target).toEqual(targetSession);
			if (call.name === 'provider_session_inspect') {
				if (inspected === 0) {
					markFirst();
					await options.firstInspectGate;
				}
				if (inspected === 1) {
					markSecond();
					await options.secondInspectGate;
				}
				const selected = options.models?.[inspected] ?? [
					{ category: 'model', currentValue: { kind: 'select', value: 'grok-4.7-low' } },
				];
				inspected += 1;
				value = {
					target: options.foreignInspect
						? { ...targetSession, sessionId: 'foreign' }
						: targetSession,
					settingsCatalog: { configOptions: selected },
				};
			} else {
				expect(call.name).toBe('events_observe');
				const events: unknown[] =
					observed === 0
						? []
						: [
								{ kind: 'turnStarted', inputId, turnId },
								{
									kind: 'itemStarted',
									item: {
										itemId: '019a0000-0000-7000-8000-000000000003',
										kind: { kind: 'userMessage' },
										text: options.prompt ?? declaration.prompt,
									},
								},
								{ kind: 'itemCompleted', itemId: '019a0000-0000-7000-8000-000000000003' },
								{
									kind: 'itemStarted',
									item: {
										itemId: '019a0000-0000-7000-8000-000000000004',
										kind: { kind: 'agentMessage' },
										text: options.reply ?? declaration.completion,
									},
								},
								{ kind: 'itemCompleted', itemId: '019a0000-0000-7000-8000-000000000004' },
								...(options.acceptedInput
									? [
											{
												kind: 'inputAccepted',
												inputId: '019a0000-0000-7000-8000-000000000005',
												turnId,
											},
										]
									: []),
								{
									kind: 'turnEnded',
									turnId,
									outcome: options.outcome ?? {
										kind: 'ended',
										stop_reason: 'endTurn',
										local_cause: null,
									},
								},
								...(options.extraTurn
									? [
											{
												kind: 'turnStarted',
												inputId: '019a0000-0000-7000-8000-000000000006',
												turnId: '019a0000-0000-7000-8000-000000000007',
											},
										]
									: []),
							];
				value = {
					target: options.foreignObservation
						? { ...targetSession, sessionId: 'foreign' }
						: targetSession,
					generation: {},
					attached: true,
					continuationGap: true,
					epoch: options.changedEpoch && observed > 0 ? 2 : 1,
					endReason: 'deadlineReached',
					events: events.map((event, index) => ({ sequence: index + 1, event })),
				};
				observed += 1;
			}
			value = { structuredContent: value };
		}
		response.writeHead(200, { 'content-type': 'application/json' });
		const reply: Record<string, unknown> = { jsonrpc: '2.0', result: value };
		reply['id'] = z.string().parse(envelope['id']);
		response.end(encode(reply));
	};
	const server = createServer((request, response) => {
		void answer(request, response).catch((error: Error) => response.destroy(error));
	});
	const listenPort = await listenOnTestPort(server);
	writeFileSync(
		join(fixture.target.service_directory, 'service.json'),
		JSON.stringify({
			serviceId: SERVICE,
			serviceEpoch: EPOCH,
			mcp: { url: `http://127.0.0.1:${listenPort}/mcp` },
		}),
	);
	const core = await createTerminalCore(120, 36);
	const hooks = cliPromptReadback({ target: fixture.target, declaration, evidence });
	const journey = promptJourney({
		provider: 'cursor',
		prompt: declaration.prompt,
		completion: declaration.completion,
		replyDeadlineMs: declaration.reply_deadline_ms,
	});
	let generation = 0;
	const driver = {
		frameGeneration: (): number => generation,
		send: async (bytes: Uint8Array): Promise<void> => {
			sent.push(Buffer.from(bytes).toString());
			generation += 1;
			core.writeRaw(
				Buffer.from(
					`\u001b[2J\u001b[H${sent.at(-1) === '\r' ? `• ${declaration.completion}\r\n› Ask Codex to do anything` : `› ${declaration.prompt}`}`,
				),
			);
		},
		waitForGrid: async (
			_scene: string,
			predicate: (grid: TerminalGrid) => boolean,
		): Promise<TerminalGrid> => {
			const grid = snapshotGrid(core);
			expect(predicate(grid), gridLines(grid).join('\n')).toBe(true);
			return grid;
		},
	};
	return {
		drive: (): Promise<string[]> =>
			driveCodexJourney(driver, journey, 'legacy', async (): Promise<void> => {}, null, hooks),
		cleanup: async (): Promise<void> => {
			core.dispose();
			await new Promise<void>((resolve, reject) =>
				server.close((error) => (error ? reject(error) : resolve())),
			);
		},
		sent,
		firstInspect,
		secondInspect,
		reads: (): string => readFileSync(join(evidence, 'exchanges.jsonl'), 'utf8'),
		result: (): unknown => decode(readFileSync(join(evidence, 'cli-mcp-readback.json'), 'utf8')),
		hasResult: (): boolean => existsSync(join(evidence, 'cli-mcp-readback.json')),
	};
}

const wrongModel = [{ category: 'model', currentValue: { kind: 'select', value: 'other-model' } }];
const correctModel = [
	{ category: 'model', currentValue: { kind: 'select', value: 'grok-4.7-low' } },
];
for (const [label, models, inputs] of [
	['initial mismatch', [wrongModel], []],
	['pre-Enter mismatch', [correctModel, wrongModel], [declaration.prompt]],
	['missing model', [[]], []],
	['ambiguous model', [[...correctModel, ...correctModel]], []],
	['boolean model', [[{ category: 'model', currentValue: { kind: 'boolean', value: true } }]], []],
] as const) {
	test(`no submitted input on ${label}`, async () => {
		const fixture = await controlledRun({ models });
		try {
			await expect(fixture.drive()).rejects.toThrow(/model/);
			expect(fixture.sent).toEqual(inputs);
			expect(fixture.hasResult()).toBe(false);
			expect(fixture.reads()).toContain('provider_session_inspect');
		} finally {
			await fixture.cleanup();
		}
	});
}

test('the first model read is awaited before any input', async () => {
	const { promise: gate, resolve: release } = Promise.withResolvers<void>();
	const fixture = await controlledRun({ firstInspectGate: gate });
	try {
		const driven = fixture.drive();
		await fixture.firstInspect;
		expect(fixture.sent).toEqual([]);
		release();
		await driven;
	} finally {
		release();
		await fixture.cleanup();
	}
});

test('the second model read is awaited immediately before submission Enter', async () => {
	const { promise: gate, resolve: release } = Promise.withResolvers<void>();
	const fixture = await controlledRun({ secondInspectGate: gate });
	try {
		const driven = fixture.drive();
		await fixture.secondInspect;
		expect(fixture.sent).toEqual([declaration.prompt]);
		release();
		await driven;
		expect(fixture.sent).toEqual([declaration.prompt, '\r']);
	} finally {
		release();
		await fixture.cleanup();
	}
});

test('passing CLI turn retains both model reads and exact MCP identity, outcome and reply', async () => {
	const fixture = await controlledRun();
	try {
		expect(await fixture.drive()).toEqual(['prompt-composed', 'prompt-replied']);
		expect(fixture.sent).toEqual([declaration.prompt, '\r']);
		expect(fixture.result()).toMatchObject({
			target: targetSession,
			inputId,
			turnId,
			reply: '24534638',
			displayedReply: '24534638',
			outcome: { kind: 'ended', stop_reason: 'endTurn', local_cause: null },
			operationIdentifier: { kind: 'unobservable' },
		});
		const reads = fixture
			.reads()
			.trim()
			.split('\n')
			.map((line) =>
				z.looseObject({ step: z.string(), tool: z.string().optional() }).parse(decode(line)),
			);
		expect(
			reads.filter((entry) => entry.tool === 'provider_session_inspect').map((entry) => entry.step),
		).toEqual(['cli-before-input-model', 'cli-before-enter-model']);
		expect(reads.filter((entry) => entry.tool === 'events_observe')).toHaveLength(2);
	} finally {
		await fixture.cleanup();
	}
});

for (const [options, reason] of [
	[{ foreignInspect: true }, 'model inspect names another Session'],
	[{ foreignObservation: true }, 'CLI MCP baseline is incomplete'],
	[{ reply: 'prose 24534638' }, 'CLI MCP reply differs from the displayed completion'],
	[{ prompt: 'foreign prompt' }, 'CLI MCP turn does not hold the exact prompt'],
	[{ extraTurn: true }, 'another input or turn shared the CLI Session'],
	[{ acceptedInput: true }, 'another input or turn shared the CLI Session'],
	[{ changedEpoch: true }, 'CLI MCP reply history is incomplete'],
	[
		{ outcome: { kind: 'ended', stop_reason: 'cancelled', local_cause: null } },
		'CLI turn did not end normally',
	],
	[{ outcome: { kind: 'lost', reason: 'providerRetired' } }, 'CLI turn did not end normally'],
] satisfies [CaseOptions, string][]) {
	test(`MCP readback refuses ${JSON.stringify(options)}`, async () => {
		const fixture = await controlledRun(options);
		try {
			await expect(fixture.drive()).rejects.toThrow(reason);
			expect(fixture.hasResult()).toBe(false);
		} finally {
			await fixture.cleanup();
		}
	});
}

test('missing or wrong provider model in a run plan fails before any output or target access', async () => {
	const { root } = await targetFixture();
	const output = join(root, 'absent-run');
	const plan = {
		repository: root,
		source_head: 'a'.repeat(40),
		driver: { path: '/absent', sha256: 'a'.repeat(64) },
		node: { path: '/absent', sha256: 'a'.repeat(64) },
		chrome: { path: '/absent', sha256: 'a'.repeat(64) },
		keys: 'legacy',
		output,
		target: (await targetFixture()).target,
		journey: { kind: 'prompt', provider: 'cursor', ...declaration },
	};
	const { expected_model: _model, ...missing } = plan.journey;
	await Promise.all(
		[missing, { ...plan.journey, expected_model: 'other-model' }].map(
			async (journey, index): Promise<void> => {
				const path = join(root, `plan-${index}.json`);
				writeFileSync(path, JSON.stringify({ ...plan, journey }));
				await expect(codexAcpMain(path, false)).rejects.toBeInstanceOf(z.ZodError);
				expect(existsSync(output)).toBe(false);
			},
		),
	);
});
