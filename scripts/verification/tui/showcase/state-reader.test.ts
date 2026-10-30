import { appendFileSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import type { ShowcaseState } from './contracts.ts';
import { ShowcaseJournal, readJournal, serializedState } from './state-reader.ts';

function frame(): ShowcaseState {
	return {
		observation: 1,
		inputOrdinal: 0,
		cause: 'frame',
		completionReady: false,
		selection: null,
		detailSelection: null,
		filter: '',
		filterCursor: 0,
		draft: '',
		cursor: 0,
		committed: '',
		choice: null,
		query: '',
		highlight: null,
		effects: [],
		setting: 'all',
		title: 'Sample',
		primaryTitle: 'Start',
		startEnabled: true,
		quitEnabled: true,
		alternatePrimary: false,
		completionCount: 0,
		pendingCompletion: false,
		surface: 'screen',
		focus: null,
		focusTargets: [],
		overlays: [],
		scroll: { main: null, dialog: null, detail: null },
		viewport: [120, 36],
		hitMap: null,
		registries: { screen: null, overlays: [] },
	};
}
function journal(records: readonly unknown[], tail = ''): string {
	const directory = mkdtempSync(join(tmpdir(), 'showcase-journal-'));
	const path = join(directory, 'showcase-state.jsonl');
	writeFileSync(path, records.map((record) => JSON.stringify(record) + '\n').join('') + tail);
	return path;
}
describe('serialized frame readback', () => {
	it('continues an owned append-only journal through a partial Unicode record', () => {
		const path = journal([{ kind: 'state', snapshot: frame() }]);
		const reader = new ShowcaseJournal(path);
		expect(reader.read().states).toHaveLength(1);
		const input = { ...frame(), observation: 2, inputOrdinal: 1, cause: 'input', title: '界' };
		const bytes = Buffer.from(JSON.stringify({ kind: 'state', snapshot: input }) + '\n');
		const boundary = bytes.indexOf(Buffer.from('界')) + 1;
		appendFileSync(path, bytes.subarray(0, boundary));
		expect(reader.read().states).toHaveLength(1);
		appendFileSync(path, bytes.subarray(boundary));
		expect(reader.read().states.at(-1)).toEqual(input);
		appendFileSync(
			path,
			JSON.stringify({ kind: 'state', snapshot: { ...frame(), observation: 3, inputOrdinal: 1 } }) +
				'\n' +
				JSON.stringify({
					kind: 'serializedFrame',
					sequence: 1,
					drawObservation: 3,
					inputOrdinal: 1,
				}) +
				'\n',
		);
		expect(
			serializedState({
				journal: reader.read(true),
				receivedFrames: 1,
				inputOrdinal: 1,
				minimumObservation: 3,
			})?.title,
		).toBe('Sample');
	});
	it.each(['rewrite', 'replacement', 'noncontiguous'] as const)(
		'rejects %s after a previously valid cached observation and latches failure',
		(kind) => {
			const path = journal([{ kind: 'state', snapshot: frame() }]),
				original = readFileSync(path);
			const reader = new ShowcaseJournal(path);
			reader.read();
			if (kind === 'rewrite')
				writeFileSync(path, Buffer.from(original.toString().replace('Sample', 'Broken')));
			else if (kind === 'replacement') {
				renameSync(path, path + '.retained');
				writeFileSync(path, original);
			} else
				appendFileSync(
					path,
					JSON.stringify({ kind: 'state', snapshot: { ...frame(), observation: 3 } }) + '\n',
				);
			expect(() => reader.read()).toThrow();
			writeFileSync(path, original);
			expect(() => reader.read()).toThrow();
		},
	);
	it('requires actual PTY arrival, matching input, and the requested draw', () => {
		const path = journal([
			{ kind: 'state', snapshot: frame() },
			{ kind: 'serializedFrame', sequence: 1, drawObservation: 1, inputOrdinal: 0 },
		]);
		const parsed = readJournal(path, true);
		const options = { journal: parsed, receivedFrames: 1, inputOrdinal: 0, minimumObservation: 1 };
		expect(serializedState(options)).toEqual(frame());
		expect(serializedState({ ...options, receivedFrames: 0 })).toBeNull();
		expect(serializedState({ ...options, inputOrdinal: 1 })).toBeNull();
		expect(serializedState({ ...options, minimumObservation: 2 })).toBeNull();
		const newer = readJournal(
			journal([
				{ kind: 'state', snapshot: frame() },
				{ kind: 'serializedFrame', sequence: 1, drawObservation: 1, inputOrdinal: 0 },
				{ kind: 'state', snapshot: { ...frame(), observation: 2 } },
				{ kind: 'serializedFrame', sequence: 2, drawObservation: 2, inputOrdinal: 0 },
			]),
			true,
		);
		expect(serializedState({ ...options, journal: newer, receivedFrames: 1 })).toBeNull();
		expect(serializedState({ ...options, journal: newer, receivedFrames: 2 })?.observation).toBe(2);
	});
	it.each([
		{ kind: 'serializedFrame', sequence: 2, drawObservation: 1, inputOrdinal: 0 },
		{ kind: 'serializedFrame', sequence: 1, drawObservation: 2, inputOrdinal: 0 },
		{ kind: 'serializedFrame', sequence: 1, drawObservation: 1, inputOrdinal: 1 },
	])('rejects broken serialization identity or draw reference %j', (record) => {
		expect(() =>
			readJournal(journal([{ kind: 'state', snapshot: frame() }, record]), true),
		).toThrow();
	});
	it('rejects callback-only proof and truncated final records', () => {
		const path = journal([{ kind: 'state', snapshot: frame() }], '{');
		expect(
			serializedState({
				journal: readJournal(path),
				receivedFrames: 1,
				inputOrdinal: 0,
				minimumObservation: 0,
			}),
		).toBeNull();
		expect(() => readJournal(path, true)).toThrow('truncated');
	});
});
