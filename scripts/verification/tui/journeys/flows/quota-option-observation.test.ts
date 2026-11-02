/** Independent closed seed census controls, separate from any alias under test. */
import { expect, test } from 'vitest';

import { requireQuotaNavigationLabels } from './quota-option-bindings.ts';
const rows = Array.from({ length: 60 }, (_, index) => String(index).padStart(2, '0') + ' Account');
test('the fixed sixty labels occur once in declared order among physical detail rows', () => {
	expect(() =>
		requireQuotaNavigationLabels(
			rows.flatMap((row) => [row, '◎ OpenAI · 1 client', 'Available', '']),
		),
	).not.toThrow();
});
test.each([
	['two-row normal fixture', rows.slice(0, 2)],
	['missing first', rows.slice(1)],
	['missing last', rows.slice(0, -1)],
	['duplicate row', [...rows.slice(0, 20), rows[19] ?? '', ...rows.slice(20)]],
	['reordered', [...rows.slice(0, 30), ...rows.slice(30).toReversed()]],
])('physical quota census rejects %s', (_name, content) => {
	expect(() => requireQuotaNavigationLabels(content)).toThrow();
});
