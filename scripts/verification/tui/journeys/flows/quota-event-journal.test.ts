/** Actual fixture envelope controls; these do not establish any quota native pass. */
import { appendFileSync, renameSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';
import { z } from 'zod';

import { temporaryRoot } from '../../fixtures/receipt-fixtures.ts';
import { QuotaEventJournal } from './quota-event-journal.ts';

const mounted = z.strictObject({
	kind: z.literal('view-mounted'),
	readback: z.strictObject({ mount: z.number() }),
});
const first =
	JSON.stringify({ sequence: 1, event: { kind: 'fixture-started', scenario: 'grouping-reopen' } }) +
	'\n';
const second =
	JSON.stringify({ sequence: 2, event: { kind: 'view-mounted', readback: { mount: 1 } } }) + '\n';
function journalFile(): string {
	const path = join(temporaryRoot(), 'quota-events.jsonl');
	writeFileSync(path, first);
	return path;
}

test('an already flushed source event after the retained boundary needs no notification', async () => {
	const path = journalFile();
	const journal = new QuotaEventJournal(path);
	const boundary = journal.read().at(-1)?.sequence ?? 0;
	appendFileSync(path, second);
	const result = await journal.waitFor({
		afterSequence: boundary,
		kind: 'view-mounted',
		signal: new AbortController().signal,
		awaitCompletion: async () => {},
		decode: (value) => mounted.parse(value),
	});
	expect(result).toEqual({ sequence: 2, event: { kind: 'view-mounted', readback: { mount: 1 } } });
	expect(journal.read(true)).toHaveLength(2);
});

test('a partial append resolves only when the genuine complete line arrives', async () => {
	const path = journalFile();
	const journal = new QuotaEventJournal(path);
	const waiting = journal.waitFor({
		afterSequence: 1,
		kind: 'view-mounted',
		signal: new AbortController().signal,
		awaitCompletion: async () => {
			appendFileSync(path, second.slice(0, -1));
			expect(journal.read()).toHaveLength(1);
			appendFileSync(path, '\n');
		},
		decode: (value) => mounted.parse(value),
	});
	expect((await waiting).sequence).toBe(2);
});

test('a consumed boundary cannot reuse a stale mount event', async () => {
	const path = journalFile();
	appendFileSync(path, second);
	const journal = new QuotaEventJournal(path);
	const stop = new AbortController();
	const waiting = journal.waitFor({
		afterSequence: 2,
		kind: 'view-mounted',
		signal: stop.signal,
		awaitCompletion: async () => {},
		decode: (value) => mounted.parse(value),
	});
	const failure = expect(waiting).rejects.toThrow('aborted');
	stop.abort();
	await failure;
	expect(() => journal.read()).toThrow('aborted');
});

test.each([
	[
		'skipped sequence',
		JSON.stringify({ sequence: 3, event: { kind: 'view-mounted' } }) + '\n',
		'contiguous',
	],
	[
		'extra envelope field',
		JSON.stringify({ sequence: 2, event: { kind: 'view-mounted' }, nativeAck: true }) + '\n',
		'',
	],
	['invalid JSON', '{bad}\n', ''],
	['invalid UTF-8', Buffer.from([0xff, 10]), ''],
] as const)('rejects %s at the actual append boundary', (_name, bytes, message) => {
	const path = journalFile();
	const journal = new QuotaEventJournal(path);
	appendFileSync(path, bytes);
	expect(() => journal.read()).toThrow(message);
});

test('the selected event body is decoded strictly before returning lifecycle facts', async () => {
	const path = journalFile();
	appendFileSync(path, second.replace('"mount":1', '"mount":"invented"'));
	const journal = new QuotaEventJournal(path);
	await expect(
		journal.waitFor({
			afterSequence: 1,
			kind: 'view-mounted',
			signal: new AbortController().signal,
			awaitCompletion: async () => {},
			decode: (value) => mounted.parse(value),
		}),
	).rejects.toThrow();
});

test.each(['replace', 'rewrite', 'truncate'] as const)(
	'rejects %s of the retained source file',
	(mutation) => {
		const path = journalFile();
		const journal = new QuotaEventJournal(path);
		journal.read();
		if (mutation === 'replace') {
			const replacement = path + '.replacement';
			writeFileSync(replacement, first);
			renameSync(replacement, path);
		} else
			writeFileSync(
				path,
				mutation === 'truncate' ? '' : first.replace('fixture-started', 'fixture-changed'),
			);
		expect(() => journal.read()).toThrow(mutation === 'replace' ? 'identity' : 'rewritten');
	},
);

test('final producer settlement rejects a truncated last record', () => {
	const path = journalFile();
	appendFileSync(path, second.slice(0, -1));
	const journal = new QuotaEventJournal(path);
	expect(() => journal.read(true)).toThrow('truncated');
});

test('a symlink cannot nominate another producer file', () => {
	const path = journalFile();
	const link = path + '.link';
	symlinkSync(path, link);
	expect(() => new QuotaEventJournal(link)).toThrow('owned regular');
});

test('future sequence floors reject instead of hiding earlier producer facts', async () => {
	const journal = new QuotaEventJournal(journalFile());
	await expect(
		journal.waitFor({
			afterSequence: 2,
			kind: 'view-mounted',
			signal: new AbortController().signal,
			awaitCompletion: async () => {},
			decode: (value) => mounted.parse(value),
		}),
	).rejects.toThrow('boundary');
});

test('an existing event cannot bypass an incomplete writer completion', async () => {
	const path = journalFile();
	appendFileSync(path, second);
	const journal = new QuotaEventJournal(path);
	const stop = new AbortController();
	const waiting = journal.waitFor({
		afterSequence: 1,
		kind: 'view-mounted',
		signal: stop.signal,
		decode: (value) => mounted.parse(value),
		awaitCompletion: (signal) =>
			new Promise<void>((_resolve, reject) => {
				signal.throwIfAborted();
				signal.addEventListener('abort', () => reject(new Error('writer cancelled')), {
					once: true,
				});
			}),
	});
	const rejected = expect(waiting).rejects.toThrow('aborted');
	stop.abort();
	await rejected;
});

test('writer failure cannot be replaced by an already flushed event', async () => {
	const path = journalFile();
	appendFileSync(path, second);
	const journal = new QuotaEventJournal(path);
	await expect(
		journal.waitFor({
			afterSequence: 1,
			kind: 'view-mounted',
			signal: new AbortController().signal,
			decode: (value) => mounted.parse(value),
			awaitCompletion: async () => {
				throw new Error('actual writer failed');
			},
		}),
	).rejects.toThrow('actual writer failed');
});
