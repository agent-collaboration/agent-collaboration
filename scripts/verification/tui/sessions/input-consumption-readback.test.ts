import { expect, test } from 'vitest';

import {
	checkInputConsumptionReceipt,
	parseInputConsumptionReadback,
} from './input-consumption-readback.ts';
import {
	inputDispatchReceiptSchema,
	inputEventReadbackSchema,
} from './input-consumption-schema.ts';
import type { InputConsumptionExpectation } from './input-consumption-schema.ts';

const press = {
	event: {
		kind: 'mouse',
		mouse: { kind: 'down', button: 'left' },
		column: 4,
		row: 7,
		modifiers: [],
	},
	surface: { kind: 'overlay', overlayId: '18446744073709551615' },
	target: null,
	action: null,
} satisfies InputConsumptionExpectation;
const release = {
	event: { kind: 'mouse', mouse: { kind: 'up', button: 'left' }, column: 4, row: 7, modifiers: [] },
	surface: { kind: 'screen' },
	target: null,
	action: null,
} satisfies InputConsumptionExpectation;
const line = (sequence: number, expected: InputConsumptionExpectation): string =>
	JSON.stringify({ sequence, ...expected }) + '\n';

test('complete journal preserves every declared event and exact u64 overlay identity', () => {
	const journal = parseInputConsumptionReadback(Buffer.from(line(1, press) + line(2, release)));
	expect(journal).toHaveLength(2);
	expect(journal[0]).toEqual({ sequence: 1, ...press });
	expect(journal[1]).toEqual({ sequence: 2, ...release });
});

test('missing, duplicated, gapped, reordered and truncated records cannot pass', () => {
	for (const bytes of [
		line(2, press),
		line(1, press) + line(1, release),
		line(1, press) + line(3, release),
		line(2, release) + line(1, press),
		line(1, press).trimEnd(),
		line(1, press) + '\n',
		'{bad json}\n',
	]) {
		expect(() => parseInputConsumptionReadback(Buffer.from(bytes))).toThrow();
	}
});

test('wrong surface, target, action, event kind and mouse phase fail independent expectation', () => {
	const receipt = inputDispatchReceiptSchema.parse({ sequence: 1, ...press });
	expect(() => checkInputConsumptionReceipt(receipt, press)).not.toThrow();
	for (const changed of [
		{ surface: { kind: 'screen' } },
		{ surface: { kind: 'overlay', overlayId: '2' } },
		{ target: 'underneath' },
		{ action: 'unexpected' },
		{ target: 'none' },
		{ event: { kind: 'resize', columns: 120, rows: 36 } },
		{ event: release.event },
	])
		expect(() => checkInputConsumptionReceipt(receipt, { ...press, ...changed })).toThrow();
});

test('wire rejects payload leaks, invalid identifiers, numeric ranges and noncanonical modifiers', () => {
	for (const change of [
		{ sequence: Number.MAX_SAFE_INTEGER + 1 },
		{ sequence: 0 },
		{ sequence: 1.5 },
		{ surface: { kind: 'overlay', overlayId: '18446744073709551616' } },
		{ surface: { kind: 'overlay', overlayId: '01' } },
		{ target: '  ' },
		{ action: '' },
		{ event: { ...press.event, column: -1 } },
		{ event: { ...press.event, row: 65536 } },
		{ event: { ...press.event, modifiers: ['control', 'shift'] } },
		{ event: { ...press.event, modifiers: ['shift', 'shift'] } },
		{
			event: {
				kind: 'key',
				code: { kind: 'text', text: 'private' },
				modifiers: [],
				phase: 'press',
			},
		},
		{ event: { kind: 'other', event: 'bracketedPaste', text: 'private' } },
		{ unexpected: true },
	])
		expect(() => inputDispatchReceiptSchema.parse({ sequence: 1, ...press, ...change })).toThrow();
	for (const code of [
		{ kind: 'shortcut', key: 'A' },
		{ kind: 'control', codepoint: 97 },
	])
		expect(() =>
			inputEventReadbackSchema.parse({ kind: 'key', code, modifiers: ['control'], phase: 'press' }),
		).toThrow();
});

test('wire accepts redacted text, actual control shortcut and non-input events', () => {
	for (const event of [
		{ kind: 'key', code: { kind: 'text' }, modifiers: ['shift'], phase: 'press' },
		{ kind: 'key', code: { kind: 'shortcut', key: 'c' }, modifiers: ['control'], phase: 'repeat' },
		{
			kind: 'key',
			code: { kind: 'control', codepoint: 3 },
			modifiers: ['control'],
			phase: 'release',
		},
		{ kind: 'key', code: { kind: 'fixed', name: 'Enter' }, modifiers: [], phase: 'press' },
		{ kind: 'other', event: 'bracketedPaste' },
		{ kind: 'other', event: 'unknown' },
		{ kind: 'resize', columns: 0, rows: 65535 },
	])
		expect(() => inputEventReadbackSchema.parse(event)).not.toThrow();
});
