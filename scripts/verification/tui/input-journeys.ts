/** Main/New input journeys bound to the returned S6 fixture outcome.
 * node scripts/verification/tui/input-journeys.ts PLAN.json [--check]
 * Requires the independently declared newRequest; pickerOnly excludes quota.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { z } from 'zod';

import {
	baselinePlanSchema,
	checkBaselineTarget,
	focusedWords,
	saveCheckpoint,
	withTerminal,
} from './baseline-journeys.ts';
import { isMain, planArgument, reportCliFailure, strictArguments } from './cli-arguments.ts';
import { openMeasuredDetails } from './dialog-frame-readiness.ts';
import { findDialog } from './grid-scene-matcher.ts';
import { encodeJourneyKey } from './journey-key.ts';
import type { PtyGridDriver, KeyProtocol } from './pty-grid-driver.ts';
import { canonicalJson, sourceBinding } from './source-binding.ts';
import { newSessionRequestSchema, readSourceQualifiedOutcome } from './source-qualified-outcome.ts';
import { cellText, gridLines, terminalTheme } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';
export const inputJourneyPlanSchema = baselinePlanSchema.extend({
	newRequest: newSessionRequestSchema.refine(
		(request) => !request.machine.startsWith('configured:'),
		'local/default New only; configured New uses the counted preparation journey',
	),
});
type InputJourneyPlan = z.infer<typeof inputJourneyPlanSchema>;
interface InputResult {
	readonly name: string;
	readonly keys: KeyProtocol;
	readonly result: 'pass' | 'red' | 'absent-at-base' | 'pending-s6';
	readonly observation: unknown;
}
function includes(grid: TerminalGrid, text: string): boolean {
	return gridLines(grid).some((line) => line.includes(text));
}
async function press(driver: PtyGridDriver, keys: KeyProtocol, key: string): Promise<void> {
	const generation = driver.frameGeneration();
	const beforeFocus =
		(key === 'Tab' || key === 'Shift+Tab') && findDialog(driver.grid(), 'New session') !== null
			? focusedWords(driver.grid(), 'New session')
			: null;
	await driver.send(encodeJourneyKey(key, keys));
	await driver.waitForGrid(
		'frame after ' + key,
		(grid) =>
			beforeFocus === null ||
			canonicalJson(focusedWords(grid, 'New session')) !== canonicalJson(beforeFocus),
		generation,
	);
}
async function finish(driver: PtyGridDriver): Promise<string> {
	const exitCode = await driver.drainUntilExit();
	const bytes = driver.bytes().toString('utf8');
	if (exitCode !== 0) throw new Error('input journey child failed');
	return bytes;
}
export function targetCell(
	grid: TerminalGrid,
	title: string,
	label: string,
): { readonly column: number; readonly row: number } {
	const rect = findDialog(grid, title);
	if (rect === null) throw new Error('dialog entry absent');
	// Button projects Focused/Primary roles; generated key hints do not. A
	// nonprimary sibling may share the actual primary button's actions row.
	const primaryColor = 0x9cbcfb; // UiTextRole::Label / TextRole::Primary.
	const hasButtonRole = (color: number | undefined): boolean =>
		color !== undefined && (color === primaryColor || color === terminalTheme.palette[11]);
	const matches = grid.cells.flatMap((row, rowIndex) =>
		row.flatMap((_cell, column) =>
			rowIndex > rect.top &&
			rowIndex < rect.bottom &&
			column > rect.left &&
			column + label.length < rect.right &&
			(row.slice(column, column + label.length).every((cell) => hasButtonRole(cell.fgRgb)) ||
				row
					.slice(rect.left + 1, rect.right)
					.some((cell) => cellText(cell) === '●' && hasButtonRole(cell.fgRgb))) &&
			row
				.slice(column, column + label.length)
				.map(cellText)
				.join('') === label
				? [{ column, row: rowIndex }]
				: [],
		),
	);
	if (matches.length !== 1 || matches[0] === undefined)
		throw new Error('drawn target missing or ambiguous: ' + label);
	return matches[0];
}
export function detailsActionCell(
	grid: TerminalGrid,
	rowKind: 'codex' | 'native-claude',
	action: 'resume' | 'fork',
): { readonly column: number; readonly row: number } {
	const label = action === 'fork' ? 'Fork' : rowKind === 'codex' ? 'Resume' : 'Open';
	return targetCell(grid, 'Session details', label);
}
async function newStart(
	plan: InputJourneyPlan,
	keys: KeyProtocol,
	activation: 'tab-enter' | 'click',
): Promise<InputResult> {
	return withTerminal({
		plan,
		keys,
		name: 'new-' + activation,
		surface: 'sessions-filters',
		sessionsReceipts: true,
		observe: async (driver, directory) => {
			await press(driver, keys, 'Ctrl+N');
			await driver.waitForGrid('New entry', (grid) => findDialog(grid, 'New session') !== null);
			const initial = focusedWords(driver.grid(), 'New session');
			saveCheckpoint(driver, directory, 'new-open');
			const focusStates: string[][] = [];
			if (activation === 'tab-enter') {
				const states: string[][] = [];
				const tab = async (count: number): Promise<void> => {
					if (count === 0) return;
					await press(driver, keys, 'Tab');
					states.push([...focusedWords(driver.grid(), 'New session')]);
					saveCheckpoint(driver, directory, 'tab-' + states.length);
					await tab(count - 1);
				};
				await tab(5);
				if (
					canonicalJson(states.at(-1)) !== canonicalJson(initial) ||
					new Set(states.map((state) => canonicalJson(state))).size !== 5
				)
					throw new Error('New Tab cycle did not reach five distinct Codex targets and wrap');
				await press(driver, keys, 'Shift+Tab');
				const reverse = focusedWords(driver.grid(), 'New session');
				await press(driver, keys, 'Tab');
				if (canonicalJson(focusedWords(driver.grid(), 'New session')) !== canonicalJson(initial))
					throw new Error('New reverse Tab did not restore focus');
				focusStates.push(...states, [...reverse]);
				// Reach the drawn Start control without assuming its index.
				const reach = async (remaining: number): Promise<void> => {
					if (focusedWords(driver.grid(), 'New session').includes('Start')) return;
					if (remaining === 0) throw new Error('Start not reachable by Tab');
					await press(driver, keys, 'Tab');
					await reach(remaining - 1);
				};
				await reach(8);
				await driver.send(encodeJourneyKey('Enter', keys));
			} else {
				const cell = targetCell(driver.grid(), 'New session', 'Start');
				await driver.send(
					Buffer.from(
						`\x1b[<0;${cell.column + 1};${cell.row + 1}M\x1b[<0;${cell.column + 1};${cell.row + 1}m`,
					),
				);
			}
			const transcript = await finish(driver);
			const outcomes = transcript.match(/SESSION_PICKER_OUTCOME [^\r\n]*/gu) ?? [];
			if (outcomes.length !== 1 || outcomes[0] !== 'SESSION_PICKER_OUTCOME start-new')
				throw new Error('Start did not produce its single safe terminal outcome');
			const returned = readSourceQualifiedOutcome(
				join(directory, 'session-receipts', 'picker-outcome.json'),
				{ kind: 'new', request: plan.newRequest },
			);
			return {
				name: 'New ' + activation,
				keys,
				result: 'pass',
				observation: {
					initial,
					focusStates,
					outcomes,
					terminalRestored: transcript.includes('\x1b[?1049l'),
					returned,
				},
			};
		},
	});
}
async function pickerEnterAndEsc(plan: InputJourneyPlan, keys: KeyProtocol): Promise<InputResult> {
	return withTerminal({
		plan,
		keys,
		name: 'main-filter-enter-esc',
		surface: 'sessions-filters',
		observe: async (driver, directory) => {
			const generation = driver.frameGeneration();
			await driver.send(Buffer.from('Codex Alpha'));
			await driver.waitForGrid('filtered Alpha', (grid) => includes(grid, '1/6'), generation);
			saveCheckpoint(driver, directory, 'filtered');
			await press(driver, keys, 'Enter');
			await driver.waitForGrid(
				'Enter inspection',
				(grid) => findDialog(grid, 'Session details') !== null,
			);
			saveCheckpoint(driver, directory, 'details');
			await press(driver, keys, 'Escape');
			await driver.waitForGrid(
				'Details closed with filter',
				(grid) => findDialog(grid, 'Session details') === null && includes(grid, '1/6'),
			);
			await press(driver, keys, 'Escape');
			await driver.waitForGrid('query cleared', (grid) => includes(grid, '6/6'));
			saveCheckpoint(driver, directory, 'query-cleared');
			await driver.send(encodeJourneyKey('Ctrl+C', keys));
			const transcript = await finish(driver);
			if (
				!transcript.includes('SESSION_PICKER_OUTCOME canceled') ||
				/SESSION_PICKER_OUTCOME (?:resume|fork|start-new)/u.test(transcript)
			)
				throw new Error('Enter/filter path dispatched a session action');
			return {
				name: 'picker Enter inspects and Esc clears query',
				keys,
				result: 'pass',
				observation: { details: true, queryCleared: true, ownCancelOutcome: true },
			};
		},
	});
}
async function quit(
	plan: InputJourneyPlan,
	keys: KeyProtocol,
	surface: 'sessions-filters' | 'quota',
	overlay: boolean,
): Promise<InputResult> {
	return withTerminal({
		plan,
		keys,
		name: 'ctrl-c-' + (overlay ? 'overlay' : 'main'),
		surface,
		observe: async (driver, directory) => {
			if (overlay) {
				await press(driver, keys, 'F1');
				await driver.waitForGrid(
					'Help entry',
					(grid) => findDialog(grid, surface === 'quota' ? 'Quota Help' : 'Help') !== null,
				);
				saveCheckpoint(driver, directory, 'help');
			}
			await driver.send(encodeJourneyKey('Ctrl+C', keys));
			const transcript = await finish(driver);
			const ownOutcome =
				surface === 'quota' ? null : transcript.includes('SESSION_PICKER_OUTCOME canceled');
			if (ownOutcome === false)
				throw new Error('picker Ctrl+C ended without its own cancel outcome');
			return {
				name: surface + ' Ctrl+C ' + (overlay ? 'Help' : 'main'),
				keys,
				result: 'pass',
				observation: {
					childExit: 0,
					ownPickerOutcome: ownOutcome,
					terminalRestored: transcript.includes('\x1b[?1049l'),
					limits:
						surface === 'quota'
							? 'Process exit and fixture effects only; committing guard requires its pending fixture at S9.'
							: null,
				},
			};
		},
	});
}
async function pickerDirectAction(
	plan: InputJourneyPlan,
	keys: KeyProtocol,
	action: 'resume' | 'fork',
): Promise<InputResult> {
	return withTerminal({
		plan,
		keys,
		name: 'main-' + action,
		surface: 'sessions-filters',
		sessionsReceipts: keys === 'legacy',
		observe: async (driver, directory) => {
			if (keys === 'kitty')
				await driver.send(Buffer.from(action === 'resume' ? '\x1b[13;5u' : '\x1b[13;6u'));
			else {
				await openMeasuredDetails(driver, directory, keys);
				// provider_filter_storyboard_request selects the native Codex Alpha
				// record first; its title and UUID also bind the outcome below.
				const cell = detailsActionCell(driver.grid(), 'codex', action);
				await driver.send(
					Buffer.from(
						`\x1b[<0;${cell.column + 1};${cell.row + 1}M\x1b[<0;${cell.column + 1};${cell.row + 1}m`,
					),
				);
			}
			if (action === 'resume') {
				const transcript = await finish(driver);
				const outcomes = transcript.match(/SESSION_PICKER_OUTCOME [^\r\n]*/gu) ?? [];
				if (
					outcomes.length !== 1 ||
					outcomes[0] !== 'SESSION_PICKER_OUTCOME resume:00000000-0000-4000-8000-000000000011'
				)
					throw new Error('resume did not return selected fixture ID');
				if (keys === 'kitty' && transcript.includes('Session details'))
					throw new Error('direct resume opened Details');
				return {
					name: 'picker ' + (keys === 'kitty' ? 'Ctrl+Enter' : 'legacy Details Resume fallback'),
					keys,
					result: 'pass',
					observation: {
						outcomes,
						limits:
							'Existing terminal marker contains UUID only; source-qualified effect identity remains a separate proof.',
					},
				};
			}
			await driver.waitForGrid(
				'Fork confirmation',
				(grid) => findDialog(grid, 'Fork session') !== null,
			);
			saveCheckpoint(driver, directory, 'fork-confirmation');
			if (/SESSION_PICKER_OUTCOME (?:resume|fork|start-new)/u.test(driver.bytes().toString('utf8')))
				throw new Error('fork dispatched before confirmation');
			await press(driver, keys, 'Escape');
			await driver.send(encodeJourneyKey('Ctrl+C', keys));
			const transcript = await finish(driver);
			if (!transcript.includes('SESSION_PICKER_OUTCOME canceled'))
				throw new Error('fork cancellation dispatched action');
			return {
				name: 'picker ' + (keys === 'kitty' ? 'Ctrl+Shift+Enter' : 'legacy Details Fork fallback'),
				keys,
				result: 'pass',
				observation: { confirmation: true, noDispatchBeforeConfirm: true, cancelled: true },
			};
		},
	});
}
async function quotaQuitKey(plan: InputJourneyPlan, keys: KeyProtocol): Promise<InputResult> {
	return withTerminal({
		plan,
		keys,
		name: 'main-q',
		surface: 'quota',
		observe: async (driver) => {
			await driver.send(Buffer.from('q'));
			const transcript = await finish(driver);
			return {
				name: 'quota Q quits',
				keys,
				result: 'pass',
				observation: { childExit: 0, terminalRestored: transcript.includes('\x1b[?1049l') },
			};
		},
	});
}
export async function runInputJourneys(plan: InputJourneyPlan): Promise<readonly InputResult[]> {
	await checkBaselineTarget(plan);
	mkdirSync(plan.output);
	const source = sourceBinding(plan.repository);
	writeFileSync(join(plan.output, 'source-binding.json'), JSON.stringify(source, null, 2));
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	const results: InputResult[] = [];
	const mode = async (keys: KeyProtocol): Promise<void> => {
		results.push(await newStart(plan, keys, 'tab-enter'));
		results.push(await newStart(plan, keys, 'click'));
		results.push(await pickerEnterAndEsc(plan, keys));
		results.push(await pickerDirectAction(plan, keys, 'resume'));
		results.push(await pickerDirectAction(plan, keys, 'fork'));
		if (!plan.pickerOnly) results.push(await quotaQuitKey(plan, keys));
		await (
			plan.pickerOnly ? (['sessions-filters'] as const) : (['sessions-filters', 'quota'] as const)
		).reduce<Promise<void>>(async (previous, surface) => {
			await previous;
			results.push(await quit(plan, keys, surface, false));
			results.push(await quit(plan, keys, surface, true));
		}, Promise.resolve());
		results.push({
			name: 'main toolbar, menu and new focus-model traversal',
			keys,
			result: 'absent-at-base',
			observation: 'S6/S7 targets absent; no full R3 claim',
		});
	};
	try {
		await mode('legacy');
		await mode('kitty');
		if (canonicalJson(sourceBinding(plan.repository)) !== canonicalJson(source))
			throw new Error('source changed during input journeys');
		return results;
	} finally {
		writeFileSync(join(plan.output, 'input-results.json'), JSON.stringify(results, null, 2));
	}
}
export async function inputJourneysMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(
		args,
		{ help: { type: 'boolean' }, check: { type: 'boolean' }, 'picker-only': { type: 'boolean' } },
		true,
	);
	if (values.help) {
		process.stdout.write(
			'input-journeys.ts PLAN.json [--check] [--picker-only]: New returned request, main Enter/Esc/Ctrl+C. newRequest is independently declared; configured preparation is a separate counted proof.\n',
		);
		return;
	}
	const plan = inputJourneyPlanSchema.parse({
		...JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
		...(values['picker-only'] ? { pickerOnly: true } : {}),
	});
	if (values.check) await checkBaselineTarget(plan);
	else await runInputJourneys(plan);
}
if (isMain(import.meta.url)) await inputJourneysMain(process.argv.slice(2)).catch(reportCliFailure);
