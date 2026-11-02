/** Example Ghostty keybinds; supplied fixture, not discovery of an installed app/config. */
import type { GhosttyKey } from './ghostty-input-encoding.ts';
export type GhosttyInputProfile = 'terminal-only' | 'example-ghostty-keybinds';
export type KeybindDelivery =
	| { readonly kind: 'encode' }
	| { readonly kind: 'consumed'; readonly action: string }
	| { readonly kind: 'replacement'; readonly bytes: Buffer };
export function applyExampleGhosttyKeybinds(
	key: GhosttyKey,
	profile: GhosttyInputProfile,
): KeybindDelivery {
	if (profile === 'terminal-only') return { kind: 'encode' };
	const consumed: Partial<Record<GhosttyKey, string>> = {
		PageUp: 'scroll_page_up',
		PageDown: 'scroll_page_down',
		Home: 'scroll_to_top',
		End: 'scroll_to_bottom',
	};
	const action = consumed[key];
	if (action !== undefined) return { kind: 'consumed', action };
	return key === 'Shift+Enter'
		? { kind: 'replacement', bytes: Buffer.from('\x0a') }
		: { kind: 'encode' };
}
