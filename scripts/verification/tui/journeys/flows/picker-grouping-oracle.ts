/** Literal R19 fixture metadata, sorted by its declared Updated timestamps, not by observed UI. */
import { canonicalJson } from '../../evidence/source-binding.ts';

export type PickerGroupingFacet = 'none' | 'started-by' | 'day' | 'folder' | 'provider' | 'machine';
interface GroupBucket {
	readonly name: string;
	readonly rows: readonly string[];
}
const alpha = 'Codex Alpha',
	beta = 'Codex Beta',
	gamma = 'Claude Gamma',
	delta = 'Claude Delta';
const cursor = 'Cursor Echo',
	zeta = 'Claude-lookalike Zeta';
const quartz = 'Hosted Codex Quartz',
	cedar = 'Hosted Codex Cedar',
	indigo = 'Hosted Codex Indigo';
// i64::MAX sorts first under Updated, although its Day bucket is Older. Missing recency sorts last.
export const pickerUngroupedOrder = [
	zeta,
	quartz,
	alpha,
	beta,
	cedar,
	indigo,
	gamma,
	delta,
	cursor,
];
export const pickerGroupingBuckets: Readonly<
	Record<Exclude<PickerGroupingFacet, 'none'>, readonly GroupBucket[]>
> = {
	'started-by': [
		{ name: 'You', rows: [alpha] },
		{ name: 'Messages', rows: [quartz, beta, indigo] },
		{ name: 'Board', rows: [cedar, gamma] },
		{ name: 'Schedules and wakes', rows: [delta] },
		{ name: 'Questions and approvals', rows: [cursor] },
		{ name: 'Thread activity', rows: [zeta] },
	],
	day: [
		{ name: 'Today', rows: [quartz, alpha] },
		{ name: 'Yesterday', rows: [beta, cedar] },
		{ name: 'This week', rows: [indigo, gamma] },
		{ name: 'Older', rows: [zeta, delta, cursor] },
	],
	folder: [
		{ name: '/fixture/a', rows: [quartz, indigo] },
		{ name: '/fixture/b', rows: [cedar] },
		{ name: '/repo/project-a', rows: [alpha, gamma, delta, cursor] },
		{ name: '/repo/project-b', rows: [beta] },
		{ name: 'Unknown folder', rows: [zeta] },
	],
	provider: [
		{ name: 'Codex', rows: [quartz, alpha, beta, cedar, indigo] },
		{ name: 'Claude Code', rows: [gamma, delta] },
		{ name: 'Cursor', rows: [cursor] },
		{ name: 'Unclassified', rows: [zeta] },
	],
	machine: [
		{ name: 'This machine', rows: [zeta, alpha, beta, gamma, delta, cursor] },
		{ name: 'Hosted Codex Quartz', rows: [quartz] },
		{ name: 'Hosted Codex Cedar', rows: [cedar] },
		{ name: 'Hosted Codex Indigo', rows: [indigo] },
	],
};

export function pickerGroupedOrder(facet: PickerGroupingFacet): readonly string[] {
	return facet === 'none'
		? pickerUngroupedOrder
		: pickerGroupingBuckets[facet].flatMap((bucket) => bucket.rows);
}

export function checkPickerGroupingObservation(
	facet: PickerGroupingFacet,
	selectedRowLabels: readonly string[],
	headings: readonly { readonly name: string; readonly count: number }[],
): readonly string[] {
	const expectedHeadings =
		facet === 'none'
			? []
			: pickerGroupingBuckets[facet].map((bucket) => ({
					name: bucket.name,
					count: bucket.rows.length,
				}));
	const failures: string[] = [];
	if (canonicalJson(selectedRowLabels) !== canonicalJson(pickerGroupedOrder(facet)))
		failures.push('grouped selectable row order differs from the nine literal fixture identities');
	if (canonicalJson(headings) !== canonicalJson(expectedHeadings))
		failures.push('group heading order, names or counts differ from the literal fixture buckets');
	return failures;
}
