import { expect, test } from 'vitest';

import type { ShowcaseRegistry } from './contracts.ts';
import { appendHelpViewport, assertHelpProjection } from './help-projection.ts';

const registry: ShowcaseRegistry = {
	scope: 'screen',
	primary: { kind: 'observed', action: null },
	actions: [
		{
			actionId: 'new',
			title: 'New session',
			toolbarLabel: null,
			keys: ['Ctrl+N', 'F2'],
			availability: { kind: 'available' },
			checked: null,
		},
		{
			actionId: 'open',
			title: 'Open selected',
			toolbarLabel: null,
			keys: [],
			availability: { kind: 'unavailable', reason: 'No selection' },
			checked: null,
		},
	],
};
const actual =
	'Context\nScreen Main\nActions\nCtrl+N, F2 New\nsession\nOpen selected No selection\nInput conventions';
test('full received Help projection rejects omitted, extra, reordered, stale-key and stale-reason rows', () => {
	expect(() => assertHelpProjection(registry, actual)).not.toThrow();
	for (const bad of [
		actual.replace('Ctrl+N', 'Ctrl+R'),
		actual.replace('Open selected', '— Open selected'),
		actual.replace('Open selected No selection\n', ''),
		actual.replace('Input conventions', 'F3 Extra\nInput conventions'),
		actual.replace('No selection', 'Offline'),
		actual.replace(
			'Ctrl+N, F2 New\nsession\nOpen selected No selection',
			'Open selected No selection\nCtrl+N, F2 New session',
		),
	])
		expect(() => assertHelpProjection(registry, bad)).toThrow('complete source registry');
	expect(() => assertHelpProjection(registry, 'Actions Ctrl+N')).toThrow('complete received Help');
});
test('physical overlap preserves repeated visible rows and refuses missing scroll intervals', () => {
	expect(
		appendHelpViewport(['Actions', 'First', 'same', 'same'], ['First', 'same', 'same', 'Last']),
	).toEqual(['Actions', 'First', 'same', 'same', 'Last']);
	expect(() => appendHelpViewport(['First'], ['Unrelated'])).toThrow('no physical overlap');
});
