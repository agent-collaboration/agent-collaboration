import { requireObservation } from './contracts.ts';
import { expectedKey } from './input-expectations.ts';
import type { ShowcaseRig } from './rig.ts';
import { effectCount, visibleTarget } from './scenario-helpers.ts';
export async function inputPhases(rig: ShowcaseRig): Promise<void> {
	await rig.rawKey({
		bytes: Buffer.from('\x1f'),
		event: {
			kind: 'key',
			code: { kind: 'shortcut', key: '7' },
			modifiers: ['control'],
			phase: 'press',
		},
		route: rig.route('unit-separator'),
		label: 'legacy U+001F / crossterm Ctrl+7 normalization',
	});
	requireObservation(
		effectCount(rig.state(), 'ctrl-seven') === 1,
		'raw unit separator matches declared Ctrl+7 exactly once',
		rig.state().effects,
	);
	await rig.burst({
		bytes: Buffer.from('\x1b[55;5:2u\x1b[55;5:3u'),
		expected: [
			expectedKey('Ctrl+7', rig.route(), 'repeat'),
			expectedKey('Ctrl+7', rig.route(), 'release'),
		],
		label: 'explicit kitty Repeat/Release supplements',
		redraw: false,
	});
	requireObservation(
		effectCount(rig.state(), 'ctrl-seven') === 1,
		'Repeat/Release cannot invoke effectful action',
		rig.state().effects,
	);
	await rig.burst({
		bytes: Buffer.from('\x1b[1;1:2B'),
		expected: [expectedKey('Down', rig.route(null, 'sample-list'), 'repeat')],
		label: 'kitty navigation Repeat supplement',
	});
	requireObservation(
		rig.state().selection === 'sample-01',
		'navigation Repeat remains usable',
		rig.state().selection,
	);
	await rig.focus('filter');
	await rig.burst({
		bytes: Buffer.from('\x1b[120;1:2u'),
		expected: [
			{
				...expectedKey('Down', rig.route(null, 'filter')),
				event: { kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'repeat' },
			},
		],
		label: 'kitty editing Repeat supplement',
	});
	requireObservation(rig.state().filter === 'x', 'editing Repeat remains usable', rig.state());
	await rig.key('Escape', rig.route(null, 'filter'));
	if (visibleTarget(rig.drawState(), 'action:record')) await rig.click('action:record', 'record');
	else await rig.menuAction('record', 'mouse');
	requireObservation(
		effectCount(rig.state(), 'open') === 1,
		'mouse press+release records one activation',
		rig.state().effects,
	);
	await rig.capture('phase-integrity');
}
