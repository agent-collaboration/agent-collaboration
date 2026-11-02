/** Literal reader controls for R17; these establish no actual CLI effect or native input. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { checkQuotaAccount } from './quota-account-checks.ts';
import { quotaObservationSchema } from './quota-live-fixture.ts';

function observation(): ReturnType<typeof quotaObservationSchema.parse> {
	const target = {
		accountId: 'effect-account-0',
		provider: 'openai',
		label: 'effect-alpha',
		position: 0,
	};
	const account = {
		accountId: 'effect-account-0',
		provider: 'openai',
		label: 'effect-alpha',
		status: 'enabled',
		credentialGeneration: 7,
	};
	const actions = [
		['login', 'Log in again', []],
		['options', 'Options…', ['Ctrl+R']],
		['floor', 'Weekly floor…', ['Ctrl+E']],
		['account-status', 'Disable', []],
		['copy-id', 'Copy account ID', []],
		['remove', 'Remove account…', []],
	].map(([identity, title, keys]) => ({
		['id']: 'quota:' + String(identity),
		title,
		keys,
		available: identity !== 'remove',
		availability:
			identity === 'remove' ? 'Unavailable { reason: "Disable the account first" }' : 'Available',
		checked: null,
	}));
	return quotaObservationSchema.parse({
		live: {
			kind: 'fixture-readback',
			requestId: 1,
			readback: {
				mount: 1,
				browseAccountId: 'effect-account-1',
				listSelection: 'effect-account-1',
				captured: { kind: 'details', ...target },
				scope: 'quota-account-actions',
				focus: 'action:quota:login',
				actions,
				published: null,
				rows: [
					{
						accountId: 'effect-account-0',
						loginState: 'NotRequired',
						label: 'effect-alpha',
						provider: 'openai',
						status: 'Account(Available)',
						enabled: true,
						weeklyWindow: 'unknown',
						detailsWeeklyWindow: 'unknown',
						resetCredits: '2 resets',
					},
				],
			},
			accounts: [account],
			accountCommands: [],
			refresh: null,
		},
		events: [],
		routerRoot: '/private/tmp/owned-fixture',
	});
}

test('enabled Remove is painted unavailable with its actual reason and no owner dispatch', async () => {
	const core = await createTerminalCore(76, 30);
	try {
		core.writeString(
			'Log in again\r\nOptions… Ctrl+R\r\nWeekly floor… Ctrl+E\r\nDisable\r\nCopy account ID\r\nRemove account… Disable the account first',
		);
		expect(
			checkQuotaAccount({
				check: 'account-actions-inventory',
				scenario: 'captured-actions',
				grid: snapshotGrid(core),
				observation: observation(),
				expectedCommands: 0,
			}),
		).toEqual([]);
	} finally {
		core.dispose();
	}
});
test.each(['available', 'missing-reason', 'dispatched'] as const)(
	'enabled Remove rejects %s',
	async (kind) => {
		const core = await createTerminalCore(76, 30);
		try {
			core.writeString(
				'Log in again\r\nOptions… Ctrl+R\r\nWeekly floor… Ctrl+E\r\nDisable\r\nCopy account ID\r\nRemove account… Disable the account first',
			);
			const value = observation(),
				remove = value.live.readback.actions.find((action) => action.id === 'quota:remove');
			if (remove === undefined) throw new Error('literal Remove absent');
			if (kind === 'available') remove.available = true;
			else if (kind === 'missing-reason') remove.availability = 'Unavailable';
			else
				value.events.push({
					sequence: 1,
					event: {
						kind: 'account-owner-started',
						command: { action: 'remove', accountId: 'effect-account-0' },
					},
				});
			expect(
				checkQuotaAccount({
					check: 'account-actions-inventory',
					scenario: 'captured-actions',
					grid: snapshotGrid(core),
					observation: value,
					expectedCommands: 0,
				}).length,
			).toBeGreaterThan(0);
		} finally {
			core.dispose();
		}
	},
);

test('real Remove completion uses the source-defined named result after emptying the only account', async () => {
	const core = await createTerminalCore(76, 30);
	try {
		core.writeString('Account removed: effect-alpha');
		const value = observation();
		value.live.accounts = [];
		value.live.readback.rows = [];
		value.live.readback.listSelection = null;
		value.live.readback.browseAccountId = null;
		value.live.readback.captured = null;
		value.live.accountCommands.push({
			command: {
				accountId: 'effect-account-0',
				label: 'effect-alpha',
				provider: 'openai',
				position: 0,
				action: 'remove',
			},
			result: { kind: 'succeeded' },
		});
		expect(
			checkQuotaAccount({
				check: 'explicit-remove-and-reselection',
				scenario: 'remove-only',
				grid: snapshotGrid(core),
				observation: value,
				expectedCommands: 1,
			}),
		).toEqual([]);
	} finally {
		core.dispose();
	}
});
