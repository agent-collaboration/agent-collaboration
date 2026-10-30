import { expect, test } from 'vitest';

import { dialogSemanticFailures } from './grid-dialog-checks.ts';
import { findDialog } from './grid-scene-matcher.ts';
import { createTerminalCore, snapshotGrid } from './terminal-grid.ts';
async function inspect(
	title: string,
	checkpoint: string,
	body: readonly string[],
): Promise<readonly string[]> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString('\x1b[2;5H┌' + '─'.repeat(105) + '┐');
		for (const [index, line] of [title, ...body].entries())
			core.writeString(`\x1b[${index + 3};5H│ ${line}\x1b[${index + 3};111H│`);
		core.writeString(`\x1b[${body.length + 4};5H└` + '─'.repeat(105) + '┘');
		const grid = snapshotGrid(core);
		const rect = findDialog(grid, title);
		expect(rect).not.toBeNull();
		return dialogSemanticFailures(grid, checkpoint, rect);
	} finally {
		core.dispose();
	}
}
test('Floor draft requires its one yellow 1% value, not guidance or another field', async () => {
	const good = [
		'東京 OpenAI Research Account Alpha Long Name [alpha-tag]',
		'Floor    \x1b[33m❯ 1%\x1b[0m',
		'0% disables protection · maximum 15%',
		'Cancel Save',
	];
	expect(await inspect('Weekly floor', 'component-quota-floor-draft', good)).toEqual([]);
	expect(
		await inspect(
			'Weekly floor',
			'component-quota-floor-draft',
			good.map((line) => line.replace('❯ 1%', '❯ 0%')),
		),
	).not.toEqual([]);
	expect(
		await inspect(
			'Weekly floor',
			'component-quota-floor-draft',
			good.map((line) => line.replace('\x1b[33m', '\x1b[37m')),
		),
	).not.toEqual([]);
});
test('contextual Account identity preserves every name character and the case-sensitive whole tag', async () => {
	const body = [
		'Selected account',
		'Account 東京 OpenAI Research Account Alpha Long Name [alpha-tag]',
		'Current quota',
		'Pool Accounts 2',
		'Close',
	];
	expect(await inspect('Quota Help', 'component-quota-confirm-help-identity', body)).toEqual([]);
	await Promise.all(
		['[ALPHA-TAG]', '[alpha-tag-extra]', '[beta-tag]'].map(async (wrong) => {
			expect(
				await inspect(
					'Quota Help',
					'component-quota-confirm-help-identity',
					body.map((line) => line.replace('[alpha-tag]', wrong)),
				),
			).not.toEqual([]);
		}),
	);
	expect(
		await inspect(
			'Quota Help',
			'component-quota-confirm-help-identity',
			body.map((line) => line.replace('Account Alpha', 'Alpha')),
		),
	).not.toEqual([]);
	expect(
		await inspect('Quota Help', 'component-quota-confirm-help-identity', [
			...body.slice(0, 2),
			body[1] ?? '',
			...body.slice(2),
		]),
	).not.toEqual([]);
});
test('confirmation requires one choice pair, identity and risk above it, and preserved Yes on Help return', async () => {
	const body = [
		'東京 OpenAI Research Account Alpha Long Name [alpha-tag]',
		'1 credit. Cannot undo.',
		'Reset quota?  No  \x1b[33mYes\x1b[0m',
		'Cancel Continue',
	];
	expect(await inspect('Confirm reset', 'component-quota-confirm-help-returned', body)).toEqual([]);
	expect(
		await inspect(
			'Confirm reset',
			'component-quota-confirm-help-returned',
			body.map((line) => line.replace('No  \x1b[33mYes', '\x1b[33mNo\x1b[0m Yes')),
		),
	).not.toEqual([]);
	expect(
		await inspect('Confirm reset', 'component-quota-confirm-help-returned', [...body, 'No']),
	).not.toEqual([]);
	expect(
		await inspect('Confirm reset', 'component-quota-confirm-help-returned', [
			body[2] ?? '',
			...body.slice(0, 2),
		]),
	).not.toEqual([]);
});
