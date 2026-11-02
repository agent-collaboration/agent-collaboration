/** Literal reader controls for R17; these establish no actual CLI effect or native input. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { checkQuotaAccount, quotaRemovalButtonAvailable } from './quota-account-checks.ts';
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
				bodyScroll: null,
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
				body: [],
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
					body: [],
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
				body: [],
			}),
		).toEqual([]);
	} finally {
		core.dispose();
	}
});

test.each([
	[
		'pinned identity and reason',
		'╭──────────────────────────────────────────────────────────╮\r\n│ Remove account effect-alpha [b350cd65]                   │\r\n│ Warning                                                  │\r\n│ Disable the account before removing it                   │\r\n╰──────────────────────────────────────────────────────────╯',
		true,
	],
	[
		'wrapped reason',
		'╭──────────────────────────────────────────────────────────╮\r\n│ Remove account effect-alpha [b350cd65]                   │\r\n│ Warning                                                  │\r\n│ Disable the account before                               │\r\n│ removing it                                              │\r\n╰──────────────────────────────────────────────────────────╯',
		true,
	],
	[
		'wrapped identity',
		'╭──────────────────────────────────────────────────────────╮\r\n│ Remove account                                           │\r\n│ effect-alpha [b350cd65]                                  │\r\n│ Warning                                                  │\r\n│ Disable the account before removing it                   │\r\n╰──────────────────────────────────────────────────────────╯',
		true,
	],
	[
		'Browse notice outside active dialog',
		'effect-alpha: Disable the account before removing it\r\n╭──────────────────────────────────────────────────────────╮\r\n│ Remove account effect-alpha                              │\r\n│ Confirm removal                                          │\r\n╰──────────────────────────────────────────────────────────╯',
		false,
	],
	['covered', 'Remove this disabled account and its credentials?', false],
	[
		'wrong account',
		'╭──────────────────────────────────────────────────────────╮\r\n│ Remove account effect-beta                               │\r\n│ Disable the account before removing it                   │\r\n╰──────────────────────────────────────────────────────────╯',
		false,
	],
	[
		'missing identity',
		'╭──────────────────────────────────────────────────────────╮\r\n│ Remove account                                           │\r\n│ Disable the account before removing it                   │\r\n╰──────────────────────────────────────────────────────────╯',
		false,
	],
])(
	'RequiresDisabled requires captured identity and exact refusal inside the active dialog: %s',
	async (_kind, painted, passes) => {
		const core = await createTerminalCore(76, 30);
		try {
			core.writeString(painted);
			const value = observation();
			value.live.readback.listSelection = 'effect-account-0';
			value.live.accountCommands.push({
				command: {
					accountId: 'effect-account-0',
					provider: 'openai',
					label: 'effect-alpha',
					position: 0,
					action: 'remove',
				},
				result: { kind: 'refused', reason: 'RequiresDisabled' },
			});
			const failures = checkQuotaAccount({
				check: 'disabled-first-refusal',
				scenario: 'remove-requires-disabled',
				grid: snapshotGrid(core),
				observation: value,
				expectedCommands: 1,
				body: [],
			});
			expect(failures).toEqual(passes ? [] : ['RequiresDisabled lacks the named visible refusal']);
		} finally {
			core.dispose();
		}
	},
);

test.each([
	['focused', '\x1b[93mRemove', true],
	['enabled', '\x1b[97mRemove', true],
	['pending', '\x1b[38;2;166;175;189mRemove', false],
	['missing', '', false],
	['duplicate', '\x1b[97mRemove\x1b[90m / \x1b[97mRemove', false],
] as const)(
	'refusal UI completion requires an actual eligible Remove label: %s',
	async (_kind, button, available) => {
		const core = await createTerminalCore(76, 30);
		try {
			core.writeString(
				'╭──────────────────────────────────────────────────────────╮\r\n│ Remove account                                           │\r\n│ \x1b[97mRemove this disabled account and its credentials?\x1b[0m         │\r\n│ \x1b[93m● Cancel\x1b[0m ' +
					button +
					'\x1b[0m                                       │\r\n╰──────────────────────────────────────────────────────────╯',
			);
			expect(quotaRemovalButtonAvailable(snapshotGrid(core))).toBe(available);
		} finally {
			core.dispose();
		}
	},
);

test.each([
	['wrapped warning', ['Warning', 'Account is no longer', 'listed'], true],
	['missing warning', ['Warning'], false],
	['different reason', ['Account is unavailable'], false],
] as const)(
	'gone captured account requires its collected painted warning: %s',
	async (_name, body, passes) => {
		const core = await createTerminalCore(32, 24);
		try {
			const value = observation();
			value.live.accounts = [];
			for (const action of value.live.readback.actions) {
				action.available = false;
				action.availability = 'Unavailable { reason: "Account is no longer listed" }';
			}
			const failures = checkQuotaAccount({
				check: 'captured-account-target',
				scenario: 'captured-actions',
				grid: snapshotGrid(core),
				observation: value,
				expectedCommands: 0,
				body,
			});
			expect(failures).toEqual(passes ? [] : ['gone captured account warning is not painted']);
		} finally {
			core.dispose();
		}
	},
);
