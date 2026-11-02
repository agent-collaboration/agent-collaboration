/** Account action oracles over actual live publication, flushed effects, and painted cells. */
import { canonicalJson } from '../../evidence/source-binding.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import type { FlowCheck } from './flow-definition.ts';
import { quotaCapturedPosition } from './quota-fixture-contract.ts';
import type { QuotaEffectScenario } from './quota-fixture-contract.ts';
import { quotaFocused, quotaPrimary } from './quota-flow-inputs.ts';
import { quotaObservationSchema } from './quota-live-fixture.ts';

export function checkQuotaAccount(options: {
	readonly check: FlowCheck;
	readonly scenario: QuotaEffectScenario;
	readonly grid: TerminalGrid;
	readonly observation: unknown;
	readonly expectedCommands: number;
}): readonly string[] {
	const { check, scenario, grid, expectedCommands } = options;
	const observed = quotaObservationSchema.parse(options.observation),
		pose = observed.live.readback;
	const position = quotaCapturedPosition(scenario),
		accountId = 'effect-account-' + position,
		label = 'effect-' + ['alpha', 'beta', 'gamma'][position];
	const target = { accountId, provider: 'openai', label, position };
	const failures: string[] = [];
	const commands = observed.live.accountCommands;
	if (check === 'account-actions-inventory') {
		const enabled = expectedCommands === 0;
		const expected = [
			['quota:login', 'Log in again'],
			['quota:options', 'Options…'],
			['quota:floor', 'Weekly floor…'],
			['quota:account-status', enabled ? 'Disable' : 'Enable'],
			['quota:copy-id', 'Copy account ID'],
			['quota:remove', 'Remove account…'],
		];
		if (
			pose.scope !== 'quota-account-actions' ||
			canonicalJson(pose.actions.map((action) => [action.id, action.title])) !==
				canonicalJson(expected)
		)
			failures.push('closed account Actions inventory/title differs');
		for (const action of pose.actions) {
			if (action.id === 'quota:remove' && enabled) {
				if (action.available || !action.availability.includes('Disable the account first'))
					failures.push('enabled Remove lost its unavailable state or actual reason');
			} else if (!action.available) failures.push('available captured account action was disabled');
		}
		const text = gridLines(grid).join('\n');
		for (const [, title] of expected)
			if (!text.includes(title ?? '')) failures.push('account menu item is not painted: ' + title);
		if (
			enabled &&
			!text.split('\n').some((line) => line.includes('Remove account…') && line.includes('Disable'))
		)
			failures.push('unavailable Remove reason is not painted');
		if (
			observed.events.filter((record) => record.event.kind === 'account-owner-started').length !==
				expectedCommands ||
			commands.length !== expectedCommands
		)
			failures.push('unavailable Remove dispatched an account command');
		return failures;
	}
	if (check === 'captured-account-target') {
		if (
			pose.captured !== null &&
			(!('accountId' in pose.captured) || pose.captured.accountId !== accountId)
		)
			failures.push('account dialog retargeted away from the captured identity');
		for (const entry of commands)
			if (
				canonicalJson({ ...entry.command, action: undefined }) !==
				canonicalJson({ ...target, action: undefined })
			)
				failures.push('account command retargeted captured identity');
		if (
			scenario === 'captured-actions' &&
			!observed.live.accounts.some((account) => account.accountId === accountId)
		) {
			const actions = pose.actions.filter((action) =>
				[
					'quota:login',
					'quota:options',
					'quota:floor',
					'quota:account-status',
					'quota:copy-id',
					'quota:remove',
				].includes(action.id),
			);
			if (
				actions.length !== 6 ||
				actions.some(
					(action) =>
						action.available || !action.availability.includes('Account is no longer listed'),
				)
			)
				failures.push('gone captured account retained an available action or lost its reason');
			if (!gridLines(grid).join(' ').includes('Account is no longer listed'))
				failures.push('gone captured account warning is not painted');
		}
	} else if (check === 'options-and-floor-captured-target') {
		if (
			pose.captured === null ||
			!['options', 'floor'].includes(pose.captured.kind) ||
			!('accountId' in pose.captured) ||
			pose.captured.accountId !== accountId ||
			pose.browseAccountId !== 'effect-account-1'
		)
			failures.push('Options/Floor did not retain captured A while Browse moved to B');
		if (
			!gridLines(grid)
				.join(' ')
				.includes(label ?? '')
		)
			failures.push('captured account label absent from the actual dialog');
	} else if (check === 'clipboard-full-account-id') {
		const writes = observed.events
			.filter((record) => record.event.kind === 'clipboard-written')
			.map((record) => record.event['payload']);
		if (canonicalJson(writes) !== canonicalJson([accountId]))
			failures.push('clipboard did not receive exactly the full captured account ID');
	} else if (check === 'status-toggle') {
		const action = expectedCommands === 2 ? 'disable' : 'enable';
		const last = commands.at(-1),
			account = observed.live.accounts.find((savedAccount) => savedAccount.accountId === accountId),
			row = pose.rows.find((publishedRow) => publishedRow.accountId === accountId);
		if (
			last?.command.action !== action ||
			last.result.kind !== 'succeeded' ||
			account?.status !== (action === 'disable' ? 'disabled' : 'enabled') ||
			row?.enabled !== (action === 'enable')
		)
			failures.push(
				'status command or published/persisted account status differs from the explicit action',
			);
	} else if (check === 'cancel-primary-and-no-remove') {
		if (commands.length !== 0) failures.push('Remove ran without explicit confirmation');
		if (
			findDialog(grid, 'Remove account') !== null &&
			(!quotaPrimary(grid, 'Remove account', 'Cancel') ||
				!quotaFocused(grid, 'Remove account', 'Cancel'))
		)
			failures.push('Cancel is not both primary and initially focused');
	} else if (check === 'explicit-remove-and-reselection') {
		const remaining = observed.live.accounts.map((account) => account.accountId),
			expected = Array.from(
				{ length: scenario === 'remove-only' ? 1 : 3 },
				(_, index) => 'effect-account-' + index,
			).filter((identity) => identity !== accountId);
		const selected = expected[Math.min(position, expected.length - 1)] ?? null;
		if (canonicalJson(remaining) !== canonicalJson(expected) || pose.listSelection !== selected)
			failures.push(
				'Remove did not publish the exact remaining accounts and old-position/last-survivor selection',
			);
		if (
			!gridLines(grid).join(' ').replace(/\s+/gu, ' ').includes(quotaRemoveCompletionMessage(label))
		)
			failures.push('Remove lacks the named visible result');
	} else if (check === 'disabled-first-refusal') {
		const last = commands.at(-1);
		if (
			last?.result.kind !== 'refused' ||
			last.result.reason !== 'RequiresDisabled' ||
			last.command.accountId !== accountId ||
			pose.listSelection !== accountId ||
			!observed.live.accounts.some(
				(account) => account.accountId === accountId && account.status === 'enabled',
			)
		)
			failures.push(
				'RequiresDisabled did not preserve the captured account with one typed refusal',
			);
	} else if (check === 'no-retry') {
		const actions =
			scenario === 'captured-actions'
				? ['copy-id', 'disable', 'enable'].slice(0, expectedCommands)
				: scenario === 'remove-cancel'
					? []
					: ['remove'];
		if (
			commands.length !== expectedCommands ||
			canonicalJson(commands.map((entry) => entry.command.action)) !== canonicalJson(actions)
		)
			failures.push(
				'account runner duplicated or changed the explicitly requested command sequence',
			);
	} else throw new Error('unbound quota account check: ' + check);
	return failures;
}

/** QuotaAccountCompletion::message names the actual removed account on Browse. */
export function quotaRemoveCompletionMessage(label: string): string {
	return 'Account removed: ' + label;
}
