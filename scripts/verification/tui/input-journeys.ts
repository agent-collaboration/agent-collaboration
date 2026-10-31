/** S0 main/New input baselines, plus the future S6 preparation-readback step.
 * node scripts/verification/tui/input-journeys.ts PLAN.json [--check]
 * Extends baselinePlanSchema with required preparationProof: pending-s6 or recorded.
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
import { encodeJourneyKey } from './capture-run.ts';
import { isMain, planArgument, reportCliFailure, strictArguments } from './cli-arguments.ts';
import { findDialog } from './grid-scene-matcher.ts';
import { preparationProofSchema, readPreparationReadback } from './preparation-readback.ts';
import type { PtyGridDriver, KeyProtocol } from './pty-grid-driver.ts';
import { canonicalJson, sourceBinding } from './source-binding.ts';
import { cellText, gridLines } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';
export const inputJourneyPlanSchema = baselinePlanSchema.extend({
	preparationProof: preparationProofSchema,
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
	const drain = async (): Promise<void> => {
		if ((await driver.receive()) !== 'eof') await drain();
	};
	await drain();
	const bytes = driver.bytes().toString('utf8');
	if ((await driver.finish()) !== 0) throw new Error('input journey child failed');
	return bytes;
}
function targetCell(
	grid: TerminalGrid,
	title: string,
	label: string,
): { readonly column: number; readonly row: number } {
	const rect = findDialog(grid, title);
	if (rect === null) throw new Error('dialog entry absent');
	const matches = grid.cells.flatMap((row, rowIndex) =>
		row.flatMap((_cell, column) =>
			rowIndex > rect.top &&
			rowIndex < rect.bottom &&
			column > rect.left &&
			column + label.length < rect.right &&
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
					throw new Error('New Tab cycle did not reach five distinct base targets and wrap');
				await press(driver, keys, 'Shift+Tab');
				const reverse = focusedWords(driver.grid(), 'New session');
				await press(driver, keys, 'Tab');
				if (canonicalJson(focusedWords(driver.grid(), 'New session')) !== canonicalJson(initial))
					throw new Error('New reverse Tab did not restore focus');
				focusStates.push(...states, [...reverse]);
				// Source baseline starts on Start. Reach it by its visible focus rather than assuming its index in later builds.
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
			if (plan.preparationProof.kind === 'recorded')
				readPreparationReadback(
					join(directory, plan.preparationProof.relativePath),
					plan.preparationProof.expected,
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
					preparation:
						plan.preparationProof.kind === 'recorded'
							? 'verified'
							: 'pending-s6: terminal outcome does not prove invocation count or fields',
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
					(grid) => findDialog(grid, surface === 'quota' ? 'Quota Help' : 'Sessions Help') !== null,
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
		observe: async (driver, directory) => {
			if (keys === 'kitty')
				await driver.send(Buffer.from(action === 'resume' ? '\x1b[13;5u' : '\x1b[13;6u'));
			else {
				await press(driver, keys, 'Enter');
				await driver.waitForGrid(
					'Details fallback',
					(grid) => findDialog(grid, 'Session details') !== null,
				);
				const cell = targetCell(
					driver.grid(),
					'Session details',
					action === 'resume' ? 'Resume' : 'Fork',
				);
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
		results.push(await quotaQuitKey(plan, keys));
		await (['sessions-filters', 'quota'] as const).reduce<Promise<void>>(
			async (previous, surface) => {
				await previous;
				results.push(await quit(plan, keys, surface, false));
				results.push(await quit(plan, keys, surface, true));
			},
			Promise.resolve(),
		);
		results.push({
			name: 'main toolbar, menu and new focus-model traversal',
			keys,
			result: 'absent-at-base',
			observation: 'S6/S7 targets absent; no full R3 claim',
		});
		if (plan.preparationProof.kind === 'pending-s6')
			results.push({
				name: 'Start once with committed values',
				keys,
				result: 'pending-s6',
				observation: 'Reader step exists; producer readback is the S6 seam, required at S9.',
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
		{ help: { type: 'boolean' }, check: { type: 'boolean' } },
		true,
	);
	if (values.help) {
		process.stdout.write(
			'input-journeys.ts PLAN.json [--check]: New Tab/click safe outcome, main Enter/Esc/Ctrl+C and future recording preparation proof. Explicit pending-s6 is never invocation proof.\n',
		);
		return;
	}
	const plan = inputJourneyPlanSchema.parse(
		JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
	);
	if (values.check) await checkBaselineTarget(plan);
	else await runInputJourneys(plan);
}
if (isMain(import.meta.url)) await inputJourneysMain(process.argv.slice(2)).catch(reportCliFailure);
