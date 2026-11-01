/** Source-bound quota artifacts are checked before adapter lookup or any output creation. */
import { verifyArtifact } from '../../evidence/source-binding.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import { quotaJourneyScenarios, quotaRequiresPrivateGuard } from './quota-fixture-contract.ts';

export function requireQuotaPlanArtifacts(plan: AuthoredFlowPlan): void {
	const required = quotaJourneyScenarios(plan.journey).some(quotaRequiresPrivateGuard);
	const standin = plan.quota?.loginStandin;
	if (required && standin === undefined)
		throw new Error('quota guard route requires a source-bound login stand-in artifact');
	if (standin !== undefined) verifyArtifact(standin);
}
