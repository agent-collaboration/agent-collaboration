/** Read received Help body rows; registry readback is independent of Help's renderer. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { cellText } from '../terminal-grid.ts';
import { childTarget, PrerequisiteGap, requireObservation } from './contracts.ts';
import type { ShowcaseRegistry } from './contracts.ts';
import type { ShowcaseRig } from './rig.ts';

const compact = (text: string): string => text.replace(/\s/gu, '');
export function assertHelpProjection(registry: ShowcaseRegistry, received: string): void {
	const text = compact(received);
	const start = text.indexOf('Actions');
	const end = text.indexOf('Inputconventions', start + 'Actions'.length);
	requireObservation(
		start >= 0 && end >= 0,
		'complete received Help Actions section is present',
		text,
	);
	const actual = text.slice(start + 'Actions'.length, end);
	const expected = registry.actions
		.map((action) =>
			compact(
				(action.keys.length === 0 ? '—' : action.keys.join(', ')) +
					action.title +
					(action.availability.kind === 'unavailable' ? action.availability.reason : ''),
			),
		)
		.join('');
	requireObservation(
		actual === expected,
		'every rendered Help entry, key and unavailable reason equals the complete source registry in order',
		{ actual, expected, actionIds: registry.actions.map((action) => action.actionId) },
	);
}
/** Down scrolls one row. Append only newly exposed physical rows, preserving duplicates. */
export function appendHelpViewport(
	previous: readonly string[],
	next: readonly string[],
): readonly string[] {
	if (previous.length === 0) return next;
	for (let overlap = Math.min(previous.length, next.length); overlap >= 1; overlap -= 1) {
		if (previous.slice(-overlap).every((row, index) => row === next[index]))
			return [...previous, ...next.slice(overlap)];
	}
	throw new PrerequisiteGap(
		'Help viewports have no physical overlap; cannot reconstruct the rendered registry',
	);
}
function helpRows(rig: ShowcaseRig): readonly string[] {
	const state = rig.drawState();
	const cells = state.hitMap?.targets.find(
		(region) =>
			region.surface === state.surface &&
			region.target === childTarget('action:help', 'help-scroll'),
	)?.cells;
	if (cells === undefined || cells.length === 0)
		throw new PrerequisiteGap('Help scroll viewport absent');
	const left = Math.min(...cells.map(([column]) => column));
	const right = Math.max(...cells.map(([column]) => column));
	const top = Math.min(...cells.map(([, row]) => row));
	const bottom = Math.max(...cells.map(([, row]) => row));
	return rig
		.grid()
		.cells.slice(top, bottom + 1)
		.map((row) => row.slice(left, right).map(cellText).join('').trimEnd());
}
export async function observeCompleteHelp(
	rig: ShowcaseRig,
	registry: ShowcaseRegistry,
): Promise<void> {
	const body = childTarget('action:help', 'help-body');
	// Called on a newly opened Help body, already at its first row. A redundant
	// Home ACK does not imply a new serialized draw when the offset is unchanged.
	const inspect = async (received: readonly string[], remaining: number): Promise<void> => {
		if (remaining === 0)
			throw new PrerequisiteGap('complete Help projection exhausted its bounded scroll budget');
		const next = helpRows(rig);
		const merged = appendHelpViewport(received, next);
		if (compact(merged.join('')).includes('Inputconventions')) {
			assertHelpProjection(registry, merged.join('\n'));
			writeFileSync(
				join(rig.directory, 'complete-help-projection.json'),
				JSON.stringify({ registry, receivedRows: merged }, null, 2),
			);
			await rig.capture('complete-help-registry-end');
			return;
		}
		if (received.length > 0 && merged.length === received.length)
			throw new PrerequisiteGap('Help stopped exposing rows before its full Actions section ended');
		await rig.key('Down', rig.route(null, body));
		await inspect(merged, remaining - 1);
	};
	await inspect([], 2048);
}
