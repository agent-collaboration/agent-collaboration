/** Literal fixture facts qualify the observer; these are not product observations. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { groupingSelectedRow, groupingVisibleHeadings } from './picker-grouping-bindings.ts';
import {
	checkPickerGroupingObservation,
	pickerGroupingBuckets,
	pickerUngroupedOrder,
} from './picker-grouping-oracle.ts';

test('each literal grouping preserves nine rows and exact named counts', () => {
	for (const facet of ['started-by', 'day', 'folder', 'provider', 'machine'] as const) {
		const groups = pickerGroupingBuckets[facet];
		const rows = groups.flatMap((group) => group.rows);
		expect(rows).toHaveLength(9);
		expect(
			checkPickerGroupingObservation(
				facet,
				rows,
				groups.map((group) => ({ name: group.name, count: group.rows.length })),
			),
		).toEqual([]);
	}
	expect(checkPickerGroupingObservation('none', pickerUngroupedOrder, [])).toEqual([]);
});

test('group observer rejects a heading treated as a row, reordered members and missing or wrong buckets', () => {
	const groups = pickerGroupingBuckets.folder;
	const rows = groups.flatMap((group) => group.rows);
	const headings = groups.map((group) => ({ name: group.name, count: group.rows.length }));
	expect(
		checkPickerGroupingObservation('folder', ['/fixture/a', ...rows.slice(1)], headings),
	).not.toEqual([]);
	expect(
		checkPickerGroupingObservation(
			'folder',
			[rows[1] ?? '', rows[0] ?? '', ...rows.slice(2)],
			headings,
		),
	).not.toEqual([]);
	expect(checkPickerGroupingObservation('folder', rows, headings.slice(1))).not.toEqual([]);
	expect(
		checkPickerGroupingObservation('folder', rows, [
			...headings.slice(0, -1),
			{ name: 'Unknown', count: 1 },
		]),
	).not.toEqual([]);
	expect(
		checkPickerGroupingObservation(
			'folder',
			rows,
			headings.map((group) => ({ name: group.name, count: group.count + 1 })),
		),
	).not.toEqual([]);
});

test('None restores the complete original sorted order and contains no group heading', () => {
	expect(checkPickerGroupingObservation('none', pickerUngroupedOrder.toReversed(), [])).not.toEqual(
		[],
	);
	expect(
		checkPickerGroupingObservation('none', pickerUngroupedOrder, [{ name: 'Codex', count: 5 }]),
	).not.toEqual([]);
});

test('the cell reader joins a wrapped heading and rejects a selected machine heading that resembles its row', async () => {
	const core = await createTerminalCore(40, 12);
	try {
		core.writeString(
			'\x1b[2;1H┌──────────────────────────────┐\x1b[3;1H│Session                       │\x1b[4;1H│ Questions and approvals      │\x1b[5;1H│ (1)                          │\x1b[6;1H│ ❯ Cursor Echo                │\x1b[10;1H└──────────────────────────────┘',
		);
		expect(groupingVisibleHeadings(snapshotGrid(core))).toEqual([
			{ name: 'Questions and approvals', count: 1 },
		]);
		expect(groupingSelectedRow(snapshotGrid(core))).toBe('Cursor Echo');
		core.writeString('\x1b[4;1H│ ❯ Hosted Codex Quartz (1)     │');
		expect(() => groupingVisibleHeadings(snapshotGrid(core))).toThrow('selection marker');
	} finally {
		core.dispose();
	}
});
