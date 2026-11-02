import { spawn } from 'node:child_process';
/** Controlled filesystem writers qualify the reader, never picker routing/effects. */
import {
	appendFileSync,
	mkdtempSync,
	readFileSync,
	realpathSync,
	renameSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { InputConsumptionReadback } from './input-consumption-readback.ts';
import type { InputConsumptionExpectation } from './input-consumption-schema.ts';

const press = {
	event: {
		kind: 'mouse',
		mouse: { kind: 'down', button: 'left' },
		column: 3,
		row: 4,
		modifiers: [],
	},
	surface: { kind: 'overlay', overlayId: '8' },
	target: null,
	action: null,
} satisfies InputConsumptionExpectation;
const release = {
	...press,
	event: { ...press.event, mouse: { kind: 'up', button: 'left' } },
	surface: { kind: 'screen' },
} satisfies InputConsumptionExpectation;
const line = (sequence: number, expected: InputConsumptionExpectation): string =>
	JSON.stringify({ sequence, ...expected }) + '\n';
// Deliberately drop every notification: only completed writer IO may wake the read.
vi.mock('node:fs', async (importOriginal) => {
	const original = await importOriginal<typeof import('node:fs')>();
	return {
		...original,
		watch: (path: string, options: { persistent: boolean }) =>
			original.watch(path, options, () => {}),
	};
});
const roots: string[] = [];
function journal(initial: string = ''): string {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'input-consumption-')));
	roots.push(root);
	const path = join(root, 'input-dispatch.jsonl');
	writeFileSync(path, initial);
	return path;
}
afterEach(() => {
	vi.useRealTimers();
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test('one retained journal binds startup prefix, ordered press/release and final exact count', async () => {
	const path = journal(
		line(1, {
			...press,
			event: { kind: 'resize', columns: 120, rows: 36 },
			surface: { kind: 'screen' },
		}),
	);
	const reader = InputConsumptionReadback.open(path);
	expect(reader.sequence).toBe(1);
	const observed = await reader.observeInput({
		expected: [press, release],
		timeoutMs: 1000,
		inject: async () => {
			appendFileSync(path, line(2, press));
			appendFileSync(path, line(3, release));
		},
	});
	expect(observed.map((record) => record.sequence)).toEqual([2, 3]);
	expect(reader.assertComplete()).toHaveLength(3);
	// A later, contiguous duplicate cannot be hidden by the earlier live prefix success.
	appendFileSync(path, line(4, release));
	expect(() => reader.assertComplete()).toThrow('unexpected input acknowledgement');
});

test('a resize must be explicitly accounted for, never silently matched to a click', async () => {
	const path = journal();
	const reader = InputConsumptionReadback.open(path);
	await expect(
		reader.observeInput({
			expected: [press],
			timeoutMs: 1000,
			inject: async () => {
				appendFileSync(
					path,
					line(1, { ...press, event: { kind: 'resize', columns: 48, rows: 24 } }),
				);
			},
		}),
	).rejects.toThrow('event mismatch');
	await expect(
		reader.observeInput({
			expected: [press],
			timeoutMs: 1000,
			inject: async () => {
				throw new Error('must not inject again');
			},
		}),
	).rejects.toThrow('event mismatch');
});

test('actual producer exit completes the receipt read with all file notifications suppressed', async () => {
	const path = journal();
	const reader = InputConsumptionReadback.open(path);
	let exitCode: number | null = null;
	const observed = await reader.observeInput({
		expected: [press],
		timeoutMs: 1000,
		inject: async () => {
			const producer = spawn(
				process.execPath,
				[
					'--input-type=module',
					'-e',
					"import{appendFileSync}from'node:fs';appendFileSync(process.argv[1],process.argv[2]);",
					path,
					line(1, press),
				],
				{ stdio: 'ignore' },
			);
			exitCode = await new Promise<number | null>((resolve, reject) => {
				producer.once('error', reject);
				producer.once('close', resolve);
			});
		},
	});
	expect(observed).toEqual([{ sequence: 1, ...press }]);
	expect(exitCode).toBe(0);
	expect(reader.assertComplete()).toHaveLength(1);
});

test('missing and partial records hit only the bounded failure guard, never a no-change pass', async () => {
	vi.useFakeTimers();
	await ['', line(1, press).slice(0, -1)].reduce<Promise<void>>(async (previous, bytes) => {
		await previous;
		const path = journal();
		const reader = InputConsumptionReadback.open(path);
		const observed = reader.observeInput({
			expected: [press],
			timeoutMs: 20,
			inject: async () => {
				appendFileSync(path, bytes);
			},
		});
		const rejection = expect(observed).rejects.toThrow('deadline');
		await vi.advanceTimersByTimeAsync(20);
		await rejection;
	}, Promise.resolve());
});

test('replaced file, rewritten prefix and pre-injection unaccounted record are rejected', async () => {
	const path = journal(line(1, press));
	const reader = InputConsumptionReadback.open(path);
	writeFileSync(path, line(1, release));
	expect(() => reader.assertComplete()).toThrow('rewritten');
	const replaced = journal();
	const otherReader = InputConsumptionReadback.open(replaced);
	renameSync(replaced, replaced + '.old');
	writeFileSync(replaced, '');
	expect(() => otherReader.assertComplete()).toThrow('identity changed');
	const extra = journal();
	const extraReader = InputConsumptionReadback.open(extra);
	appendFileSync(extra, line(1, press));
	let injections = 0;
	await expect(
		extraReader.observeInput({
			expected: [press],
			timeoutMs: 1000,
			inject: async () => {
				injections += 1;
			},
		}),
	).rejects.toThrow('unexpected input acknowledgement');
	expect(injections).toBe(0);
});

test('duplicate sequence, extra acknowledgement and action mismatch fail the live observer', async () => {
	await [
		line(1, press) + line(1, release),
		line(1, press) + line(2, press),
		line(1, { ...press, action: 'underneath' }),
	].reduce<Promise<void>>(async (previous, bytes) => {
		await previous;
		const path = journal();
		const reader = InputConsumptionReadback.open(path);
		await expect(
			reader.observeInput({
				expected: [press],
				timeoutMs: 1000,
				inject: async () => {
					appendFileSync(path, bytes);
				},
			}),
		).rejects.toThrow();
	}, Promise.resolve());
});

test('abort before injection, symlink and truncated initial journal cannot produce evidence', async () => {
	const path = journal();
	const reader = InputConsumptionReadback.open(path);
	const controller = new AbortController();
	controller.abort();
	let injections = 0;
	await expect(
		reader.observeInput({
			expected: [press],
			timeoutMs: 1000,
			signal: controller.signal,
			inject: async () => {
				injections += 1;
			},
		}),
	).rejects.toThrow();
	expect(injections).toBe(0);
	const link = journal();
	renameSync(link, link + '.target');
	symlinkSync(link + '.target', link);
	expect(() => InputConsumptionReadback.open(link)).toThrow('owned regular');
	expect(() => InputConsumptionReadback.open(journal(line(1, press).trimEnd()))).toThrow(
		'incomplete',
	);
	expect(readFileSync(path, 'utf8')).toBe('');
});

test('abort after registration prevents a queued injection and a null rejection stays failure', async () => {
	const path = journal();
	const reader = InputConsumptionReadback.open(path);
	const controller = new AbortController();
	let injections = 0;
	const observed = reader.observeInput({
		expected: [press],
		timeoutMs: 1000,
		signal: controller.signal,
		inject: async () => {
			injections += 1;
		},
	});
	controller.abort();
	await expect(observed).rejects.toThrow('aborted');
	expect(injections).toBe(0);
	const other = InputConsumptionReadback.open(journal());
	await expect(
		other.observeInput({ expected: [press], timeoutMs: 1000, inject: () => Promise.reject(null) }),
	).rejects.toThrow('failed');
});
