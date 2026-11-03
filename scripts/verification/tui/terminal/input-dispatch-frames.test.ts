/** Controlled bytes qualify the frame observer; no product outcome is claimed. */
import { expect, test } from 'vitest';

import {
	DispatchFrameIndex,
	dispatchFrameGrid,
	IncrementalDispatchFrameReplay,
} from './input-dispatch-frames.ts';
import { gridLines } from './terminal-grid.ts';

const frame = (text: string): string => '\x1b[?2026h\x1b[2J\x1b[H' + text + '\x1b[?2026l';
const marker = (sequence: number): string => `\x1b]7770;dispatch=${sequence}\x1b\\`;
for (const destination of ['first', 'one-row-wrong', 'last']) {
	test(`stale queued frame cannot replace marked destination ${destination}, including no-op`, async () => {
		const stale = frame('first');
		const bytes = Buffer.from(stale + frame(destination) + marker(1) + frame('later unrelated'));
		const index = new DispatchFrameIndex();
		// A file ACK could arrive here, while this stale frame is still queued.
		index.append(Buffer.from(stale));
		expect(index.get(1)).toBeNull();
		for (const byte of bytes.subarray(Buffer.byteLength(stale))) index.append(Buffer.from([byte]));
		const binding = index.get(1);
		if (binding === null) throw new Error('controlled marker absent');
		expect(binding.frameGeneration).toBe(2);
		const grid = await dispatchFrameGrid(bytes, [], binding);
		expect(gridLines(grid)[0]?.trim()).toBe(destination);
		expect(gridLines(grid).join('')).not.toContain('later unrelated');
		// A wrong one-row result remains observable; the reader never waits for last.
		expect(gridLines(grid)[0]?.trim() === 'last').toBe(destination === 'last');
	});
}
test('markers require a completed frame or adjacent cohort and exact contiguous input sequence', () => {
	for (const bytes of [
		marker(1),
		frame('x') + marker(2),
		frame('x') + marker(1) + 'unmarked output' + marker(2),
		frame('x') + marker(1) + frame('y') + marker(1),
		'\x1b[?2026h' + marker(1),
		frame('x') + '\x1b]7770;dispatch=0\x1b\\',
	]) {
		expect(() => new DispatchFrameIndex().append(Buffer.from(bytes))).toThrow();
	}
});
test('adjacent contiguous markers share only the state after the whole batch, never a later unmarked frame', async () => {
	const bytes = Buffer.from(
		frame('after both inputs') + marker(1) + marker(2) + frame('later unrelated'),
	);
	const index = new DispatchFrameIndex();
	for (const byte of bytes) index.append(Buffer.from([byte]));
	const first = index.get(1),
		last = index.get(2);
	if (first === null || last === null) throw new Error('batch marker absent');
	expect(first.frameEndByteOffset).toBe(last.frameEndByteOffset);
	expect(first.frameGeneration).toBe(last.frameGeneration);
	for (const grid of await Promise.all(
		[first, last].map((binding) => dispatchFrameGrid(bytes, [], binding)),
	)) {
		expect(gridLines(grid)[0]?.trim()).toBe('after both inputs');
		expect(gridLines(grid).join('')).not.toContain('later unrelated');
	}
});
test('marked replay retains UTF-8 byte boundaries and the kernel resize geometry', async () => {
	const first = Buffer.from(frame('wide entry'));
	const bytes = Buffer.concat([
		first,
		Buffer.from(frame('❯ Claude · 📂') + marker(1) + frame('later')),
	]);
	const index = new DispatchFrameIndex();
	index.append(bytes);
	const binding = index.get(1);
	if (binding === null) throw new Error('controlled marker absent');
	const grid = await dispatchFrameGrid(
		bytes,
		[{ byteOffset: first.length, columns: 32, rows: 24 }],
		binding,
	);
	expect(grid).toMatchObject({ columns: 32, rows: 24 });
	expect(gridLines(grid)[0]?.trim()).toBe('❯ Claude · 📂');
});

test('only an explicit next handoff ACK permits an unframed sequence; it never invents a frame', () => {
	const index = new DispatchFrameIndex();
	index.acknowledgeLoopEndingHandoff(1);
	expect(index.get(1)).toBeNull();
	expect(index.isUnframedHandoff(1)).toBe(true);
	expect(() => index.acknowledgeLoopEndingHandoff(1)).toThrow('next exact');
	expect(() => index.acknowledgeLoopEndingHandoff(3)).toThrow('next exact');
	index.append(Buffer.from(frame('fresh quota') + marker(2)));
	expect(index.get(2)?.sequence).toBe(2);
	expect(() => index.append(Buffer.from(frame('skipped') + marker(4)))).toThrow('not contiguous');
});
test('an optional real loop-ending marker stays real and cannot be silently renumbered', () => {
	const index = new DispatchFrameIndex();
	index.append(Buffer.from(frame('ending') + marker(1)));
	index.acknowledgeLoopEndingHandoff(1);
	expect(index.isUnframedHandoff(1)).toBe(false);
	index.append(Buffer.from(frame('remount') + marker(2)));
	expect(index.get(1)?.sequence).toBe(1);
	expect(index.get(2)?.sequence).toBe(2);
	expect(() => index.append(Buffer.from(frame('duplicate') + marker(2)))).toThrow('not contiguous');
});
test('an optional marker for an explicitly acknowledged handoff may finish arriving before the next input marker', () => {
	const index = new DispatchFrameIndex();
	index.acknowledgeLoopEndingHandoff(1);
	index.append(Buffer.from(frame('ending') + marker(1)));
	expect(index.isUnframedHandoff(1)).toBe(false);
	index.append(Buffer.from(frame('fresh quota') + marker(2)));
	expect(index.get(2)?.sequence).toBe(2);
	expect(() => index.append(Buffer.from(frame('duplicate') + marker(1)))).toThrow('not contiguous');
});

test('incremental marked frames equal full replay across edits, UTF-8, cohorts and resizes', async () => {
	const first = Buffer.from(frame('entry') + marker(1));
	const second = Buffer.concat([
		first,
		Buffer.from('\x1b[?2026h\x1b[2;1H❯ Claude · 📂\x1b[?2026l' + marker(2) + marker(3)),
	]);
	const bytes = Buffer.concat([
		second,
		Buffer.from('\x1b[?2026h\x1b[3;1Hlast\x1b[?2026l' + marker(4) + frame('future')),
	]);
	const resizes = [
		{ byteOffset: first.length, columns: 76, rows: 30 },
		{ byteOffset: second.length, columns: 32, rows: 24 },
	];
	const index = new DispatchFrameIndex();
	index.append(bytes);
	const replay = new IncrementalDispatchFrameReplay();
	try {
		await [1, 2, 3, 4].reduce(async (previous, sequence) => {
			await previous;
			const binding = index.get(sequence);
			if (binding === null) throw new Error('literal marker absent');
			const observed = await replay.grid(bytes, resizes, binding);
			expect(observed).toEqual(await dispatchFrameGrid(bytes, resizes, binding));
			expect(await replay.grid(bytes, resizes, binding)).toBe(observed);
		}, Promise.resolve());
		const old = index.get(1);
		if (old === null) throw new Error('literal first absent');
		await expect(replay.grid(bytes, resizes, old)).rejects.toThrow('stream order');
	} finally {
		await replay.close();
	}
});
test('full-replay equivalence rejects a dropped content byte in the incremental stream', async () => {
	const bytes = Buffer.from(frame('HELLO') + marker(1));
	const index = new DispatchFrameIndex();
	index.append(bytes);
	const binding = index.get(1);
	if (binding === null) throw new Error('literal marker absent');
	const position = bytes.indexOf(Buffer.from('HELLO'));
	const dropped = Buffer.concat([
		bytes.subarray(0, position),
		bytes.subarray(position + 1),
		Buffer.from([0]),
	]);
	const replay = new IncrementalDispatchFrameReplay();
	try {
		const expected = await dispatchFrameGrid(bytes, [], binding);
		await expect(
			(async () => {
				expect(await replay.grid(dropped, [], binding)).toEqual(expected);
			})(),
		).rejects.toThrow();
	} finally {
		await replay.close();
	}
});

for (const [description, bytes, expectedGeneration] of [
	['one mode pair', frame('one'), 1],
	['repeated set and reset', '\x1b[?2026h' + frame('one') + '\x1b[?2026l', 1],
	['two distinct frames', frame('one') + frame('two'), 2],
] as const) {
	test(`synchronized output counts real transitions: ${description}`, async () => {
		const stream = Buffer.from(bytes + marker(1));
		const index = new DispatchFrameIndex();
		for (const byte of stream) index.append(Buffer.from([byte]));
		const binding = index.get(1);
		if (binding === null) throw new Error('expected marked frame absent');
		expect(binding.frameGeneration).toBe(expectedGeneration);
		expect(gridLines(await dispatchFrameGrid(stream, [], binding))[0]?.trim()).toBe(
			expectedGeneration === 2 ? 'two' : 'one',
		);
	});
}

test('a redundant reset alone cannot manufacture a completed frame for a dispatch', () => {
	const index = new DispatchFrameIndex();
	expect(() => index.append(Buffer.from('\x1b[?2026l'))).not.toThrow();
	expect(() => index.append(Buffer.from(marker(1)))).toThrow('lacks its own complete frame');
});
