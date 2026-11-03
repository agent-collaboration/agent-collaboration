/** Pure quota adapter registry; construction creates no output, socket or product process. */
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type { NativeFlowBindings } from './native-flow-runtime.ts';
import { quotaAccountBindings } from './quota-account-bindings.ts';
import { quotaJourneyScenarios } from './quota-fixture-contract.ts';
import type { QuotaEffectScenario } from './quota-fixture-contract.ts';
import { quotaFixtureLayout } from './quota-fixture-layout.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';
import { quotaGroupingBindings } from './quota-grouping-bindings.ts';
import { QuotaLiveFixture } from './quota-live-fixture.ts';
import { quotaLoginBindings } from './quota-login-bindings.ts';
import { quotaRefreshBindings } from './quota-refresh-bindings.ts';

export interface QuotaFlowBundle {
	readonly bindings: NativeFlowBindings;
	readonly layout: QuotaFixtureLayout;
	readonly fixture: QuotaLiveFixture;
}
export function quotaFlowBundle(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	scenarioOverride?: QuotaEffectScenario,
): QuotaFlowBundle | null {
	const name = invocation.journey.journeyId;
	if (
		![
			'quota-actions-every-command',
			'quota-remove-cancel-primary',
			'quota-remove-middle',
			'quota-remove-last',
			'quota-remove-only',
			'quota-remove-requires-disabled-refusal',
			'quota-login-succeeded',
			'quota-login-cancelled-130',
			'quota-login-failed',
			'quota-group-by-f6',
		].includes(name) &&
		!name.startsWith('quota-refresh-')
	)
		return null;
	const allowed = quotaJourneyScenarios(name);
	const scenario = scenarioOverride ?? allowed[0];
	if (scenario !== undefined && !allowed.includes(scenario))
		throw new Error('quota phase scenario is outside authored journey');
	if (scenario === undefined) throw new Error('supported quota flow has no scenario');
	const layout = quotaFixtureLayout({
		scenario,
		product: plan.product,
		...(plan.quota === undefined ? {} : { standin: plan.quota.loginStandin }),
		repository: plan.observerRepository,
		evidence,
	});
	const fixture = new QuotaLiveFixture(layout),
		options = { plan, invocation, evidence, layout, fixture };
	const bindings =
		quotaLoginBindings(options) ??
		quotaGroupingBindings(options) ??
		quotaAccountBindings(options) ??
		quotaRefreshBindings(options);
	if (bindings === null) throw new Error('declared quota adapter disappeared');
	return { bindings, layout, fixture };
}
