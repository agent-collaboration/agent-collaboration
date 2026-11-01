/** Authored S9 journeys. Absent entries are coverage states, never passing runs. */
import { z } from 'zod';

import { journeyGridSizes } from '../journey-definition.ts';

export const flowBaseHead = 'e23f28aff392d7c09905fe65ae64628ddaba81a4';
export const flowDesignHead = '76803a480b21cad31dc32db1b6fbc86c31aab61b';
/** Lead contract 01a19430: sequential steps observe their final input; a burst
 * observes only its final state. Author separate steps for intermediate states.
 */
export const flowObservationRule =
	'separate steps for per-input state; bursts prove only state after their final input';
export const flowCheckSchema = z.enum([
	'picker-toolbar',
	'quota-toolbar',
	'closed-menu-inventory',
	'projection-parity',
	'help-conventions-and-menu-control',
	'checked-settings',
	'new-run-mode-by-capability',
	'right-dropoff',
	'no-bottom-actions',
	'filter-focus-and-live-query',
	'menu-key-and-tab',
	'stopped-claude-open-only',
	'captured-session-outcome',
	'dropdown-highlight-uncommitted',
	'single-local-machine',
	'compound-start-once',
	'returned-new-once',
	'configured-start-once',
	'legacy-start-fallback',
	'login-primary-by-status',
	'exact-login-command',
	'command-after-terminal-restore',
	'command-result-and-fresh-return',
	'captured-account-target',
	'account-actions-inventory',
	'options-and-floor-captured-target',
	'clipboard-full-account-id',
	'status-toggle',
	'cancel-primary-and-no-remove',
	'explicit-remove-and-reselection',
	'disabled-first-refusal',
	'refresh-one-in-flight',
	'refresh-visible-and-disabled',
	'refresh-values-and-details',
	'typed-refresh-result',
	'no-retry',
	'option-alias-delivery',
	'picker-option-page-and-ends',
	'content-scroll-option-page-and-ends',
	'help-option-page-and-ends',
]);
export type FlowCheck = z.infer<typeof flowCheckSchema>;
export const flowStepSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('press'), key: z.string().min(1) }),
	z.strictObject({ kind: z.literal('click'), target: z.string().min(1) }),
	z.strictObject({
		kind: z.literal('focus'),
		target: z.string().min(1),
		via: z.enum(['Tab', 'Shift+Tab', 'arrows']),
	}),
	z.strictObject({ kind: z.literal('type'), text: z.string() }),
	z.strictObject({
		kind: z.literal('capture'),
		checkpoint: z.string().min(1),
		checks: z.array(flowCheckSchema).min(1),
	}),
	z.strictObject({
		kind: z.literal('readback'),
		port: z.enum([
			'preparation',
			'account-command',
			'clipboard',
			'quota-refresh',
			'source-qualified-outcome',
		]),
		checks: z.array(flowCheckSchema).min(1),
	}),
	z.strictObject({
		kind: z.literal('fixture'),
		operation: z.string().min(1),
		purpose: z.string().min(1),
	}),
]);
export const flowJourneySchema = z.strictObject({
	journeyId: z.string().regex(/^[a-z0-9-]+$/u),
	requirements: z.array(z.enum(['U4/R4', 'U12/R13', 'U5/R9', 'U16/R17', 'U17/R18'])).min(1),
	surface: z.enum(['sessions', 'quota']),
	availability: z.strictObject({
		state: z.literal('absent-at-base'),
		head: z.literal(flowBaseHead),
		introducedBy: z.enum(['S6', 'S7']),
	}),
	reference: z.strictObject({
		revision: z.literal(flowDesignHead),
		specification: z.string().min(1),
		verificationDesign: z.string().min(1),
	}),
	grids: z.array(z.enum(journeyGridSizes)).min(1),
	keys: z.array(z.enum(['legacy', 'kitty'])).min(1),
	inputProfile: z.enum(['terminal-only', 'example-ghostty-keybinds']).optional(),
	fixtures: z.array(z.string().min(1)),
	steps: z.array(flowStepSchema).min(1),
	wrongOutcomes: z.array(z.string().min(1)).min(1),
	admissionInputs: z.array(z.string().min(1)),
});
export type FlowJourney = z.infer<typeof flowJourneySchema>;
export type FlowStep = z.infer<typeof flowStepSchema>;

export function flowReference(
	specification: string,
	verificationDesign: string,
): FlowJourney['reference'] {
	return { revision: flowDesignHead, specification, verificationDesign };
}
export function absentAtBase(introducedBy: 'S6' | 'S7'): FlowJourney['availability'] {
	return { state: 'absent-at-base', head: flowBaseHead, introducedBy };
}
