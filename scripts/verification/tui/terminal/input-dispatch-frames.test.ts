/** Controlled bytes qualify the frame observer; no product outcome is claimed. */
import { expect, test } from 'vitest';

import { DispatchFrameIndex, dispatchFrameGrid } from './input-dispatch-frames.ts';
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
