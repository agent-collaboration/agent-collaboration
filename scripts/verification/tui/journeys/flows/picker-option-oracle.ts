/** Literal fixture order and physical page budgets, read from the declared large-list source. */
import { optionPickerHelpCensus } from '../scenes/option-help-census.ts';
import type { SessionNavigationRow } from '../scenes/option-navigation-observation.ts';

export const pickerOptionRows: readonly SessionNavigationRow[] = Array.from(
	{ length: 60 },
	(_, index) => {
		const label = 'Large session ' + String(index + 1).padStart(3, '0');
		return { label, visiblePrefix: label };
	},
);
// Outer border 2 + toolbar 1 + list border 2 + column heading 1.
// Normal records occupy 3 rows; below 40 columns they occupy 4 rows.
export const pickerOptionPageRows = {
	'120x36': 10,
	'76x30': 8,
	'48x12': 2,
	'32x24': 4,
} as const;

const groupingPosition = optionPickerHelpCensus.indexOf('Sort: Created') + 1;
if (groupingPosition === 0)
	throw new Error('historical Help census lost the declared grouping insertion');
/** Current registry retains the historical census and inserts the R19 actions after Sort. */
export const currentPickerHelpCensus: readonly string[] = [
	...optionPickerHelpCensus.slice(0, groupingPosition),
	'F6 Group by',
	'Group by: None',
	'Group by: Started by',
	'Group by: Day',
	'Group by: Folder',
	'Group by: Provider',
	'Group by: Machine',
	...optionPickerHelpCensus.slice(groupingPosition),
];

export function requirePickerOptionIndex(actual: number, expected: number): void {
	if (actual !== expected)
		throw new Error(
			`picker page destination differs: expected row ${expected}, observed ${actual}`,
		);
}
