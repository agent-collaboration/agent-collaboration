/** S7's declared fixture wire; these bindings alone establish no native observation. */
import { z } from 'zod';

export const quotaEffectEntry =
	'quota::quota_account_adapter::tests::effectful_fixture::compiled_quota_effectful_fixture';
export const quotaEffectArguments = [
	'--exact',
	quotaEffectEntry,
	'--ignored',
	'--nocapture',
	'--test-threads=1',
] as const;
export const quotaEffectScenarioSchema = z.enum([
	'captured-actions',
	'grouping-reopen',
	'remove-cancel',
	'remove-middle',
	'remove-last',
	'remove-only',
	'remove-held',
	'remove-requires-disabled',
	'login-succeeded',
	'login-cancelled130',
	'login-failed7',
	'refresh-all-success',
	'refresh-partial',
	'refresh-all-failed',
	'refresh-setup-failed',
]);
export type QuotaEffectScenario = z.infer<typeof quotaEffectScenarioSchema>;

const commonConfiguration = z.strictObject({
	scenario: quotaEffectScenarioSchema,
	outputDirectory: z.string().startsWith('/'),
	events: z.string().startsWith('/'),
	receipt: z.string().startsWith('/'),
	controlSocket: z.string().startsWith('/'),
	dispatchReceipt: z.string().startsWith('/'),
});
export const quotaEffectConfigurationSchema = z.union([
	commonConfiguration.extend({
		scenario: z.enum(['login-succeeded', 'login-cancelled130', 'login-failed7', 'grouping-reopen']),
		loginReceipt: z.string().startsWith('/'),
		loginGateSocket: z.string().startsWith('/'),
		loginStandin: z.string().startsWith('/'),
	}),
	commonConfiguration.refine((value) => !quotaRequiresPrivateGuard(value.scenario)),
]);
export type QuotaEffectConfiguration = z.infer<typeof quotaEffectConfigurationSchema>;

export function quotaLoginExit(scenario: QuotaEffectScenario): 0 | 130 | 7 | null {
	switch (scenario) {
		case 'login-succeeded':
			return 0;
		case 'login-cancelled130':
			return 130;
		case 'login-failed7':
			return 7;
		case 'captured-actions':
		case 'grouping-reopen':
		case 'remove-cancel':
		case 'remove-middle':
		case 'remove-last':
		case 'remove-only':
		case 'remove-held':
		case 'remove-requires-disabled':
		case 'refresh-all-success':
		case 'refresh-partial':
		case 'refresh-all-failed':
		case 'refresh-setup-failed':
			return null;
	}
	const exhaustive: never = scenario;
	throw new Error('unknown quota scenario: ' + String(exhaustive));
}
export function quotaRequiresPrivateGuard(scenario: QuotaEffectScenario): boolean {
	return quotaLoginExit(scenario) !== null || scenario === 'grouping-reopen';
}
export function quotaCapturedPosition(scenario: QuotaEffectScenario): number {
	return scenario === 'remove-middle' || scenario === 'remove-held'
		? 1
		: scenario === 'remove-last'
			? 2
			: 0;
}
export function quotaAccountCount(scenario: QuotaEffectScenario): number {
	return scenario === 'remove-only'
		? 1
		: ['remove-middle', 'remove-last', 'remove-held'].includes(scenario)
			? 3
			: 2;
}
/** Whole parent journeys may need two independent invocations, never a second refresh in one child. */
export function quotaJourneyScenarios(journey: string): readonly QuotaEffectScenario[] {
	const fixed: Readonly<Record<string, QuotaEffectScenario>> = {
		'quota-actions-every-command': 'captured-actions',
		'quota-group-by-f6': 'grouping-reopen',
		'quota-remove-cancel-primary': 'remove-cancel',
		'quota-remove-middle': 'remove-middle',
		'quota-remove-last': 'remove-last',
		'quota-remove-only': 'remove-only',
		'quota-remove-requires-disabled-refusal': 'remove-requires-disabled',
		'quota-login-succeeded': 'login-succeeded',
		'quota-login-cancelled-130': 'login-cancelled130',
		'quota-login-failed': 'login-failed7',
	};
	const scenario = fixed[journey];
	if (journey === 'quota-remove-middle') return ['remove-middle', 'remove-held'];
	if (scenario !== undefined) return [scenario];
	const refresh = quotaEffectScenarioSchema.safeParse(journey.replace(/^quota-/u, ''));
	return refresh.success && refresh.data.startsWith('refresh-') ? [refresh.data, refresh.data] : [];
}
