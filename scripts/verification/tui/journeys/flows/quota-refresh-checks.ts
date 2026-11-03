/** Typed mapper, request and visible-value obligations for one real refresh invocation. */
import { canonicalJson } from '../../evidence/source-binding.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import type { FlowCheck } from './flow-definition.ts';
import { checkQuotaRefreshReceipt } from './quota-effect-readback.ts';
import type { QuotaEffectScenario } from './quota-fixture-contract.ts';
import { quotaObservationSchema } from './quota-live-fixture.ts';
import type { QuotaLiveReadback } from './quota-live-fixture.ts';
export function checkQuotaRefresh(options: {
	readonly check: FlowCheck;
	readonly scenario: QuotaEffectScenario;
	readonly phase: 'before' | 'pending' | 'completed';
	readonly baseline: QuotaLiveReadback;
	readonly observation: unknown;
	readonly grid: TerminalGrid;
	readonly body: readonly string[];
}): readonly string[] {
	const { check, scenario, phase, baseline, grid, body } = options,
		observed = quotaObservationSchema.parse(options.observation),
		live = observed.live,
		refresh = live.refresh,
		failures: string[] = [];
	if (refresh === null) return ['actual refresh receipt absent'];
	if (
		live.readback.mount !== baseline.readback.mount ||
		live.readback.listSelection !== 'effect-account-0' ||
		live.readback.browseAccountId !== 'effect-account-0'
	)
		failures.push('refresh changed mount or selected identity');
	if (phase === 'before') return [...failures, 'refresh did not reach the held invocation'];
	if (check === 'refresh-visible-and-disabled') {
		const action = live.readback.actions.find((item) => item.id === 'quota:refresh'),
			text = gridLines(grid).join(' ');
		if (
			action === undefined ||
			action.available ||
			!action.availability.includes('Refreshing') ||
			!text.includes('Refreshing')
		)
			failures.push('pending refresh lost its visible unavailable state/reason');
	}
	if (phase === 'pending') {
		if (
			refresh.serviceCalls !== 1 ||
			refresh.released ||
			refresh.callsWhileHeld !== 0 ||
			refresh.observations.length !== 0 ||
			observed.events.filter((record) => record.event.kind === 'refresh-owner-started').length !== 1
		)
			failures.push('pending refresh duplicated or completed before release');
		return failures;
	}
	if (check === 'typed-refresh-result' || check === 'refresh-one-in-flight' || check === 'no-retry')
		failures.push(...checkQuotaRefreshReceipt(scenario, refresh));
	if (check === 'refresh-values-and-details') {
		if (body.length === 0) failures.push('actual refreshed body census absent');
		const rows = live.readback.rows;
		if (
			canonicalJson(rows.map((row) => row.accountId)) !==
			canonicalJson(['effect-account-0', 'effect-account-1'])
		)
			failures.push('refresh lost/reordered account identities');
		for (const [index, row] of rows.entries()) {
			const changed =
				scenario === 'refresh-all-success' || (scenario === 'refresh-partial' && index === 0);
			const before = baseline.readback.rows.find(
				(previous) => previous.accountId === row.accountId,
			);
			if (before === undefined) throw new Error('refresh baseline row absent');
			if (
				changed
					? !/\b9%/u.test(row.weeklyWindow) ||
						!/\b9%/u.test(row.detailsWeeklyWindow) ||
						row.weeklyWindow === before.weeklyWindow
					: row.weeklyWindow !== before.weeklyWindow ||
						row.detailsWeeklyWindow !== before.detailsWeeklyWindow
			)
				failures.push('refreshed report/Details values differ from per-account outcomes');
		}
		const text = body.join(' ').replace(/\s+/gu, ' ');
		if (scenario === 'refresh-all-success' || scenario === 'refresh-partial') {
			if (!/\b9%/u.test(text))
				failures.push('actual painted quota body lacks the refreshed weekly value');
		} else if (text.includes('9%')) failures.push('failed refresh painted a successful value');
		if (live.readback.captured?.kind === 'details') {
			if (live.readback.captured.accountId !== 'effect-account-0')
				failures.push('refresh retargeted open Details');
			const row = rows[0];
			if (
				row !== undefined &&
				!text.replace(/\s/gu, '').includes(row.detailsWeeklyWindow.replace(/\s/gu, ''))
			)
				failures.push('open Details does not paint its reloaded weekly facts');
		} else {
			const notice =
				scenario === 'refresh-all-success'
					? 'Refreshed'
					: scenario === 'refresh-partial'
						? 'Partial refresh'
						: 'Refresh failed';
			if (!gridLines(grid).join(' ').replace(/\s+/gu, ' ').includes(notice))
				failures.push('typed result lost its actual visible result message');
		}
	}
	return failures;
}
