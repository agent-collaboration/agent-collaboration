/** Source-qualified previous-focus owner controls; no product or native menu is launched. */
import { expect, test } from 'vitest';

import { checkInputConsumptionReceipt } from '../../sessions/input-consumption-readback.ts';
import { pickerMenuNavigationExpectation } from './picker-menu-observation.ts';

const surface = { kind: 'overlay', overlayId: '1' } as const;
test('menu Down belongs to the action focused before navigation, not null or the following action', () => {
	const expected = pickerMenuNavigationExpectation({ key: 'Down', focusedIndex: 0, surface });
	const actual = {
		sequence: 1,
		event: { kind: 'key', code: { kind: 'fixed', name: 'Down' }, modifiers: [], phase: 'press' },
		surface,
		target: 'action:new',
		action: null,
	};
	expect(() => checkInputConsumptionReceipt(actual, expected)).not.toThrow();
	for (const target of [null, 'action:focus-filter', 'action:quit'])
		expect(() => checkInputConsumptionReceipt({ ...actual, target }, expected)).toThrow(
			'target mismatch',
		);
	expect(() => checkInputConsumptionReceipt({ ...actual, action: 'new' }, expected)).toThrow(
		'action mismatch',
	);
});

test('menu Tab uses the same previous-focus owner, including checked choices and wraparound', () => {
	expect(pickerMenuNavigationExpectation({ key: 'Tab', focusedIndex: 11, surface })).toMatchObject({
		target: 'action:group-none',
		action: null,
	});
	expect(pickerMenuNavigationExpectation({ key: 'Down', focusedIndex: 25, surface }).target).toBe(
		'action:quit',
	);
	expect(pickerMenuNavigationExpectation({ key: 'Down', focusedIndex: 26, surface }).target).toBe(
		'action:new',
	);
	expect(pickerMenuNavigationExpectation({ key: 'Down', focusedIndex: 27, surface }).target).toBe(
		'action:focus-filter',
	);
});

test('menu owner lookup refuses an invalid focus index or a key that the menu does not consume', () => {
	for (const focusedIndex of [-1, 0.5, Number.NaN])
		expect(() => pickerMenuNavigationExpectation({ key: 'Down', focusedIndex, surface })).toThrow();
	expect(() => pickerMenuNavigationExpectation({ key: 'F12', focusedIndex: 0, surface })).toThrow();
});
