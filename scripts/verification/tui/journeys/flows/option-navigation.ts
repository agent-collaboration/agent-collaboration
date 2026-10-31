/** Owner decision 2026-10-31, push 01a191b2-bfee-7452-87f9-691c69261adf.
 * Authored until a declared alias-enabled S6/S7 target and independent seeds exist.
 */
import { absentAtBase, flowGridSizes, flowReference } from './flow-definition.ts';
import type { FlowJourney, FlowCheck } from './flow-definition.ts';

export const optionNavigationJourneys: readonly FlowJourney[] = (
	['picker-list', 'quota-list', 'sessions-help', 'quota-help'] as const
).map((context): FlowJourney => {
	const surface = context === 'picker-list' || context === 'sessions-help' ? 'sessions' : 'quota';
	const help = context.endsWith('help');
	const check: FlowCheck = help
		? 'help-option-page-and-ends'
		: context === 'picker-list'
			? 'picker-option-page-and-ends'
			: 'content-scroll-option-page-and-ends';
	return {
		journeyId: context + '-option-navigation',
		requirements: ['U12/R13'],
		surface,
		availability: absentAtBase(surface === 'sessions' ? 'S6' : 'S7'),
		reference: flowReference(
			'R13 navigation; owner 2026-10-31 Option arrows page and Option+Shift arrows first/last',
			'example Ghostty keybind profile; owner push 01a191b2-bfee-7452-87f9-691c69261adf',
		),
		inputProfile: 'example-ghostty-keybinds',
		grids: [...flowGridSizes],
		keys: ['legacy', 'kitty'],
		fixtures: [
			'fixed ordered unique session/account identities with more than two viewport pages',
			'independently declared first/last identities and literal page destinations for each grid, not derived from output',
			'Help with independently declared first/middle/last lines and enough text to scroll',
			'current selected identity, query and overlay state; no real provider operations',
		],
		steps: [
			{
				kind: 'fixture',
				operation: 'seed-option-navigation-' + context,
				purpose:
					'Pin row order and per-grid expected page destinations before launch; quota keeps ContentScroll policy, picker keeps RowPaging.',
			},
			{ kind: 'focus', target: help ? 'main list before Help' : 'main list', via: 'Tab' },
			...(help ? ([{ kind: 'press', key: 'F1' }] satisfies FlowJourney['steps']) : []),
			{ kind: 'capture', checkpoint: 'alias-entry', checks: [check] },
			{
				kind: 'fixture',
				operation: 'try-consumed-original-navigation-keys',
				purpose:
					'Through GhosttyJourneyInput(example-ghostty-keybinds), PageUp/PageDown/Home/End each returns consumed and sends zero bytes. Do not await an impossible app ACK/frame or count this as an app navigation pass.',
			},
			{ kind: 'press', key: 'Alt+Shift+Up' },
			{ kind: 'capture', checkpoint: 'alias-first', checks: ['option-alias-delivery', check] },
			{ kind: 'press', key: 'Alt+Down' },
			{ kind: 'capture', checkpoint: 'alias-page-down', checks: ['option-alias-delivery', check] },
			{ kind: 'press', key: 'Alt+Up' },
			{ kind: 'capture', checkpoint: 'alias-page-up', checks: ['option-alias-delivery', check] },
			{ kind: 'press', key: 'Alt+Shift+Down' },
			{ kind: 'capture', checkpoint: 'alias-last', checks: ['option-alias-delivery', check] },
			{
				kind: 'fixture',
				operation: 'resize-same-child-to-each-declared-grid',
				purpose:
					'Retain PID/UID/birth/executable and identities, read actual modes again, recompute page budget, repeat four aliases and compare each literal per-grid destination.',
			},
			...(help
				? ([
						{ kind: 'press', key: 'Escape' },
						{
							kind: 'capture',
							checkpoint: 'alias-help-return',
							checks: ['help-option-page-and-ends'],
						},
					] satisfies FlowJourney['steps'])
				: []),
		],
		wrongOutcomes: [
			'alias consumed by supplied keybind model or encoded as an unmodified arrow',
			'Alt stripped because macos-option-as-alt is unset',
			'wrong Shift bit or kitty press-event suffix',
			'write acknowledgment counted as app consumption or movement',
			'picker moves one row instead of a page, or misses exact first/last identity',
			'quota changes selected identity while scrolling, or pages selection instead of content',
			'Help alias reaches the covered list or changes selection/query',
			'Help closing loses original focus, query, selection or scroll',
		],
		admissionInputs: [
			'Exact alias-enabled S6/S7 head, signed product/driver, fixed independent navigation seed and per-grid oracle.',
			'Each key must traverse the example profile and actual PTY-fed modes; retain delivery hex, exact Alt/Alt+Shift Up/Down dispatch ACK and completed visible movement.',
			'Picker page changes selected identity by the declared page budget; quota ContentScroll and Help move content by their declared body viewport while selection stays unchanged. Compare first/last and page destinations independently, not against another implementation-derived alias result.',
		],
	};
});
