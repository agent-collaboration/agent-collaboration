/** Normalized wire expectations from the pinned producer and crossterm Display. */
import type {
	InputConsumptionExpectation,
	InputEventReadback,
} from '../sessions/input-consumption-schema.ts';
import { PrerequisiteGap } from './contracts.ts';
export interface ExpectedRoute {
	readonly surface: string;
	readonly target: string | null;
	readonly action: string | null;
}
export function expectedClick(options: {
	readonly point: { readonly column: number; readonly row: number };
	readonly pressRoute: ExpectedRoute;
	readonly releaseSurface: string;
}): readonly InputConsumptionExpectation[] {
	return (
		[
			['down', options.pressRoute],
			['up', { surface: options.releaseSurface, target: null, action: null }],
		] as const
	).map(([kind, route]) =>
		expectedInput(
			{
				kind: 'mouse',
				mouse: { kind, button: 'left' },
				column: options.point.column,
				row: options.point.row,
				modifiers: [],
			},
			route,
		),
	);
}
export function expectedInput(
	event: InputEventReadback,
	route: ExpectedRoute,
): InputConsumptionExpectation {
	const surface =
		route.surface === 'screen'
			? ({ kind: 'screen' } as const)
			: ({ kind: 'overlay', overlayId: route.surface.replace(/^overlay:/u, '') } as const);
	return { event, surface, target: route.target, action: route.action };
}
export function expectedKey(
	key: string,
	route: ExpectedRoute,
	phase: 'press' | 'repeat' | 'release' = 'press',
): InputConsumptionExpectation {
	const modifiers: ('shift' | 'control' | 'alt')[] = [];
	if (key.includes('Shift+')) modifiers.push('shift');
	if (key.includes('Ctrl+')) modifiers.push('control');
	if (key.includes('Alt+')) modifiers.push('alt');
	const base = key.split('+').at(-1);
	if (base === undefined) throw new PrerequisiteGap('empty logical key');
	const fixed: Readonly<Record<string, string>> = {
		Enter: process.platform === 'darwin' ? 'Return' : 'Enter',
		Escape: 'Esc',
		'Shift+Tab': 'Back Tab',
		PageUp: 'Page Up',
		PageDown: 'Page Down',
		Backspace: process.platform === 'darwin' ? 'Delete' : 'Backspace',
		Delete: process.platform === 'darwin' ? 'Fwd Del' : 'Del',
	};
	const code =
		modifiers.includes('control') && base.length === 1
			? ({ kind: 'shortcut', key: base.toLowerCase() } as const)
			: ({ kind: 'fixed', name: fixed[key] ?? fixed[base] ?? base } as const);
	return expectedInput({ kind: 'key', code, modifiers, phase }, route);
}
