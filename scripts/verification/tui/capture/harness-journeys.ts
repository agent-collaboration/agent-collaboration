import { flowBaseHead } from '../journeys/flows/flow-definition.ts';
import { journeyGridSizes } from '../journeys/journey-definition.ts';
/** S9 authoring contract; byte qualification is not a product or physical-terminal pass. */
import type { GhosttyKey } from '../terminal/ghostty-input-encoding.ts';
export const layerCKeyboardKeys = [
	'Ctrl+Enter',
	'Ctrl+Shift+Enter',
	'Ctrl+C',
	'Escape',
	'F10',
	'Tab',
	'Shift+Enter',
	'Up',
	'Down',
	'Alt+Up',
	'Alt+Down',
	'Alt+Shift+Up',
	'Alt+Shift+Down',
	'Left',
	'Right',
	'PageUp',
	'PageDown',
	'Home',
	'End',
] as const satisfies readonly GhosttyKey[];
export const layerCProfiles = ['terminal-only', 'example-ghostty-keybinds'] as const;
export const layerCPointerJourneys = (['sessions', 'quota'] as const).map((surface) => ({
	journeyId: surface + '-ghostty-pointer-matrix',
	availability: {
		state: 'absent-at-base',
		head: flowBaseHead,
		introducedBy: surface === 'sessions' ? 'S6' : 'S7',
	},
	grids: journeyGridSizes,
	keys: ['legacy', 'kitty'],
	profiles: layerCProfiles,
	steps: [
		'read the actual keyboard flags, cursor mode, mouse tracking and wire format from PTY-fed core',
		'click an enabled main control using SGR1006 press and release; observe its independently specified action once',
		'wheel up then down over scrollable content; assert independently declared rows and selection semantics',
		'open overlay through the same input adapter; retain captured identity, query, draft and selected row',
		'click and wheel at an underlying target while overlay is open; causally acknowledge input processing before asserting no underlying change',
		'outside dialog stays open; outside menu/dropdown closes only that overlay and never activates the target beneath',
		'resize the SAME retained PTY child to the next grid; wait for actual completed redraw and retained identity',
		'read modes again, recompute points from current grid and repeat click press/release and both wheel directions inside and beneath overlay',
	],
	fixtures: [
		'real delivered input-consumption acknowledgement for negative no-change claims; write ACK alone is insufficient',
		'independent semantic targets/current grid coordinates and expected effects',
		'one retained child and kernel resize receipts',
	],
	wrongOutcomes: [
		'mouse sent while reporting disabled',
		'invented SGR mode',
		'pixel mode treated as cells',
		'missing release',
		'stale coordinates after resize',
		'input reaches covered target',
		'absence of redraw mistaken for proof of inertness',
	],
}));
