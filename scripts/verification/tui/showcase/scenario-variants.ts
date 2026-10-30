/** Independent children keep an early contrary outcome from hiding later bounded probes. */
import type { ScenarioId } from './contracts.ts';
export type ScenarioVariant =
	| { readonly kind: 'ordinary' }
	| { readonly kind: 'quit'; readonly surface: 'main' | 'menu' | 'dropdown' | 'dialog' }
	| { readonly kind: 'lifecycle'; readonly transition: 'close' | 'help' | 'late' }
	| {
			readonly kind: 'wide';
			readonly scene: 'menu' | 'form' | 'dropdown' | 'edge';
			readonly glyph: 'folder' | 'mail' | 'cjk';
	  };
export function variantsForScenario(scenarioId: ScenarioId): readonly ScenarioVariant[] {
	if (scenarioId === 'SC07')
		return (['main', 'menu', 'dropdown', 'dialog'] as const).map((surface) => ({
			kind: 'quit',
			surface,
		}));
	if (scenarioId === 'SC26')
		return (['close', 'help', 'late'] as const).map((transition) => ({
			kind: 'lifecycle',
			transition,
		}));
	if (scenarioId === 'SC25')
		return (['menu', 'form', 'dropdown', 'edge'] as const).flatMap((scene) =>
			(['folder', 'mail', 'cjk'] as const).map((glyph) => ({ kind: 'wide', scene, glyph })),
		);
	return [{ kind: 'ordinary' }];
}
export function variantLabel(variant: ScenarioVariant): string {
	switch (variant.kind) {
		case 'ordinary':
			return 'ordinary';
		case 'quit':
			return 'quit-' + variant.surface;
		case 'lifecycle':
			return 'lifecycle-' + variant.transition;
		case 'wide':
			return 'wide-' + variant.scene + '-' + variant.glyph;
	}
	throw new Error('unknown scenario variant');
}
