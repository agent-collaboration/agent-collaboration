/** Visible-value controls use literal state and emulator bytes, never a claimed native refresh. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { quotaObservationSchema } from './quota-live-fixture.ts';
import { checkQuotaRefresh } from './quota-refresh-checks.ts';
function observation(percentage: string): ReturnType<typeof quotaObservationSchema.parse> {
	return quotaObservationSchema.parse({
		routerRoot: '/private/tmp/fixture',
		events: [],
		live: {
			kind: 'fixture-readback',
			requestId: 1,
			accounts: [],
			accountCommands: [],
			refresh: {
				serviceCalls: 1,
				callsWhileHeld: 1,
				released: true,
				observations: [],
				requests: [],
			},
			readback: {
				mount: 1,
				browseAccountId: 'effect-account-0',
				listSelection: 'effect-account-0',
				captured: null,
				scope: 'quota-browse',
				focus: 'action:quota:accounts',
				actions: [],
				published: { frame: 5 },
				rows: [0, 1].map((index) => ({
					accountId: 'effect-account-' + index,
					label: index === 0 ? 'effect-alpha' : 'effect-beta',
					provider: 'openai',
					enabled: true,
					status: 'Account(Available)',
					loginState: 'NotRequired',
					weeklyWindow: percentage + ' left',
					detailsWeeklyWindow: 'week ' + percentage + ' left',
					resetCredits: '2 resets',
				})),
			},
		},
	});
}
test.each([
	['refreshed paint', '9%', '9% Weekly Refreshed', true],
	['stale paint', '9%', '?% Weekly Refreshed', false],
	['wrong reloaded value', '99%', '9% Weekly Refreshed', false],
	['lost result notice', '9%', '9% Weekly', false],
])(
	'refresh values reject stale or mismatched facts: %s',
	async (_name, percentage, paint, accepted) => {
		const core = await createTerminalCore(76, 30);
		try {
			core.writeString(paint);
			const value = observation(percentage);
			const failures = checkQuotaRefresh({
				check: 'refresh-values-and-details',
				scenario: 'refresh-all-success',
				phase: 'completed',
				baseline: observation('?%').live,
				observation: value,
				grid: snapshotGrid(core),
				body: [paint],
			});
			expect(failures.length === 0).toBe(accepted);
		} finally {
			core.dispose();
		}
	},
);
