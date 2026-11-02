/** Independent base Help census, classified by R13 at 76803a480.
 * Source: picker_help_view.rs::render_help and retained S0 Help captures.
 */
export interface ActionProjection {
	readonly name: string;
	readonly keys: readonly string[];
}
export type PickerSetting = 'folderScope' | 'runtimeView' | 'sort';
interface BaseHelpEntry {
	readonly name: string;
	readonly keys: readonly string[];
	readonly placement:
		| { readonly kind: 'action'; readonly action: ActionProjection }
		| { readonly kind: 'setting'; readonly setting: PickerSetting }
		| { readonly kind: 'help-convention' };
}
const action = (
	name: string,
	keys: readonly string[],
	actionName: string = name,
): BaseHelpEntry => ({
	name,
	keys,
	placement: { kind: 'action', action: { name: actionName, keys } },
});
const setting = (name: string, key: string, settingKind: PickerSetting): BaseHelpEntry => ({
	name,
	keys: [key],
	placement: { kind: 'setting', setting: settingKind },
});
const convention = (name: string, keys: readonly string[]): BaseHelpEntry => ({
	name,
	keys,
	placement: { kind: 'help-convention' },
});
export const basePickerHelpEntries: readonly BaseHelpEntry[] = [
	convention('Move selection', ['↑', '↓']),
	convention('Move a page', ['PgUp', 'PgDn']),
	convention('First / last choice', ['Home', 'End']),
	action('Open details', ['Enter', 'F4'], 'Details'),
	action('Resume selected session', ['Ctrl+Enter'], 'Open'),
	action('Fork selected session', ['Ctrl+Shift+Enter'], 'Fork'),
	action('New Codex session', ['Ctrl+N'], 'New session…'),
	action('Choose machine', ['Ctrl+G', 'F2']),
	action('Session filters', ['Ctrl+F', 'F3']),
	setting('Cycle scope', 'Ctrl+S', 'folderScope'),
	setting('Cycle runtime view', 'Ctrl+T', 'runtimeView'),
	setting('Cycle sort', 'Ctrl+O', 'sort'),
	action('Refresh inventory', ['Ctrl+R']),
	convention('Search by session ID, branch or repository', ['id:', 'b:', 'repo:']),
	convention('Search sessions', ['Typing', 'Backspace']),
	convention('Clear search or go back', ['Esc']),
	action('Open or close Help', ['F1', 'Ctrl+/'], 'Help'),
	convention('Close Help', ['Esc']),
	action('Exit picker', ['Ctrl+C', 'Ctrl+D'], 'Quit'),
];
export const pickerBaseActions: readonly ActionProjection[] = basePickerHelpEntries.flatMap(
	(entry) => (entry.placement.kind === 'action' ? [entry.placement.action] : []),
);
export const pickerHelpConventions: readonly ActionProjection[] = basePickerHelpEntries
	.filter((entry) => entry.placement.kind === 'help-convention')
	.map((entry) => ({ name: entry.name, keys: entry.keys }));
export const pickerMenuControl: ActionProjection = { name: 'Menu', keys: ['F10'] };
export const pickerNewMenuItem = 'New background session…';
// Value identities are from R13 (folder) and the retained runtime/sort enums.
// Display names must equal across projections; no unapproved prefix is imposed here.
export const pickerSettingValues = {
	folderScope: { keys: ['Ctrl+S'], values: ['This folder', 'All'] },
	runtimeView: { keys: ['Ctrl+T'], values: ['All', 'Blocked', 'Active', 'Idle'] },
	sort: { keys: ['Ctrl+O'], values: ['updated', 'created'] },
} as const;
