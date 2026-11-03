import { expect, test } from 'vitest';

import type { ShowcaseState } from './contracts.ts';
import { eligibleVisibleFocus } from './scenario-helpers.ts';

function focusState(
	focus: string,
	painted: string,
	eligible = true,
): Pick<ShowcaseState, 'focus' | 'focusTargets' | 'hitMap' | 'surface'> {
	return {
		focus,
		surface: 'screen',
		focusTargets: [{ targetId: focus, role: 'focusable', eligible }],
		hitMap: {
			frame: 1,
			surfaces: [],
			targets: [{ surface: 'screen', target: painted, kind: 'scrollArea', cells: [[1, 2]] }],
		},
	};
}
test('list keyboard focus uses the public scroll child while ordinary targets remain exact', () => {
	expect(() =>
		eligibleVisibleFocus(focusState('sample-list', '11:sample-list:scroll')),
	).not.toThrow();
	expect(() => eligibleVisibleFocus(focusState('filter', 'filter'))).not.toThrow();
	expect(() => eligibleVisibleFocus(focusState('filter', '11:sample-list:scroll'))).toThrow();
	expect(() => eligibleVisibleFocus(focusState('action:record', 'action:record', false))).toThrow();
	expect(() => eligibleVisibleFocus(focusState('sample-list', 'unrelated'))).toThrow();
	const state = focusState('sample-list', '11:sample-list:scroll');
	expect(() => eligibleVisibleFocus({ ...state, hitMap: null })).toThrow();
	expect(() =>
		eligibleVisibleFocus({
			...state,
			hitMap: {
				frame: 1,
				surfaces: [],
				targets: [
					{
						surface: 'overlay:1',
						target: '11:sample-list:scroll',
						kind: 'scrollArea',
						cells: [[1, 2]],
					},
				],
			},
		}),
	).toThrow();
});
