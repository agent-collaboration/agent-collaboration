/** Independent closed seed census controls, separate from any alias under test. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { quotaContentViewport } from './quota-content-census.ts';
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

test('quota content geometry excludes the column heading and separator above its actual scrollbar', async () => {
	const core = await createTerminalCore(40, 12);
	try {
		core.writeString(
			'\x1b[2;2H╭──────────────────╮\x1b[3;2H│Account           │\x1b[4;2H│──────────────────│\x1b[5;2H│First            █│\x1b[6;2H│Second           ││\x1b[7;2H│Third            ││\x1b[8;2H╰──────────────────╯',
		);
		const viewport = quotaContentViewport(snapshotGrid(core));
		expect(viewport.rows).toEqual(['First', 'Second', 'Third']);
		expect(viewport.scrollbar).toEqual({ column: 19, top: 4, bottom: 6 });
	} finally {
		core.dispose();
	}
});
