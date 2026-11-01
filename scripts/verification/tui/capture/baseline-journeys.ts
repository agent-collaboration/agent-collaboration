import { execFileSync } from 'node:child_process';
/** Pre-UI baseline at the Lead-declared build. Known defects, regressions and
 * absent future entries are separate results; an entry failure is never red.
 * node scripts/verification/tui/capture/baseline-journeys.ts PLAN.json [--check]
 */
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { isMain, reportCliFailure } from '../cli/cli-arguments.ts';
import { findDialog } from '../journeys/scenes/grid-scene-matcher.ts';
import { InputConsumptionReadback } from '../sessions/input-consumption-readback.ts';
import type { InputConsumptionExpectation } from '../sessions/input-consumption-schema.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import type { KeyProtocol } from '../terminal/pty-grid-driver.ts';
import { cellText, gridLines, terminalTheme } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { qualifyTerminal } from '../terminal/terminal-qualification.ts';

export const baselinePlanSchema = z.object({
	repository: z.string(),
	productSourceHead: z.string(),
	driver: z.object({ path: z.string(), sha256: z.string().length(64) }),
	product: z.object({ path: z.string(), sha256: z.string().length(64) }),
	output: z.string(),
	pickerOnly: z.boolean().default(false),
});
export type BaselinePlan = z.infer<typeof baselinePlanSchema>;
interface BaselineResult {
	readonly name: string;
	readonly keys: KeyProtocol;
	readonly kind: 'known-defect' | 'isolation' | 'new-entry';
	readonly result: 'red' | 'pass' | 'absent-at-base' | 'entry-pending-fixture';
	readonly expected: string;
	readonly observed: unknown;
}
function keyBytes(
	protocol: KeyProtocol,
	key: 'escape' | 'help' | 'new' | 'details' | 'tab' | 'down' | 'end',
): Buffer {
	const legacy = {
		escape: '\x1b',
		help: '\x1bOP',
		new: '\x0e',
		details: '\x1bOS',
		tab: '\t',
		down: '\x1b[B',
		end: '\x1b[F',
	};
	const kitty = {
		escape: '\x1b[27u',
		help: '\x1b[1;1P',
		new: '\x1b[110;5u',
		details: '\x1b[1;1S',
		tab: '\x1b[9u',
		down: '\x1b[1;1B',
		end: '\x1b[1;1F',
	};
	return Buffer.from((protocol === 'legacy' ? legacy : kitty)[key]);
}
export function focusedWords(grid: TerminalGrid, title: string): readonly string[] {
	const rect = findDialog(grid, title);
	if (rect === null) throw new Error(`dialog entry missing: ${title}`);
	return grid.cells
		.slice(rect.top, rect.bottom + 1)
		.map((row) =>
			row
				.slice(rect.left, rect.right + 1)
				.filter((cell) => cell.fgRgb === terminalTheme.palette[11])
				.map(cellText)
				.join('')
				.trim(),
		)
		.filter(Boolean);
}
function hasText(grid: TerminalGrid, text: string): boolean {
	return gridLines(grid).some((line) => line.includes(text));
}
export function saveCheckpoint(driver: PtyGridDriver, directory: string, name: string): void {
	writeFileSync(join(directory, `${name}.ansi`), driver.bytes());
	writeFileSync(join(directory, `${name}.grid.json`), JSON.stringify(driver.grid(), null, 2));
	writeFileSync(join(directory, `${name}.txt`), `${gridLines(driver.grid()).join('\n')}\n`);
}
export async function withTerminal<TResult>(options: {
	readonly plan: BaselinePlan;
	readonly keys: KeyProtocol;
	readonly name: string;
	readonly surface: 'sessions-filters' | 'quota';
	readonly sessionsReceipts?: boolean;
	readonly observe: (driver: PtyGridDriver, directory: string) => Promise<TResult>;
}): Promise<TResult> {
	const directory = join(options.plan.output, `${options.keys}-${options.surface}-${options.name}`);
	mkdirSync(directory);
	const receipts = options.sessionsReceipts ? join(directory, 'session-receipts') : undefined;
	if (receipts !== undefined) mkdirSync(receipts);
	const driver = await PtyGridDriver.start({
		repository: options.plan.repository,
		driver: options.plan.driver.path,
		surface: options.surface,
		keys: options.keys,
		evidence: directory,
		...(receipts === undefined ? {} : { sessionsReceiptDirectory: receipts }),
	});
	try {
		await driver.waitForGrid('main', (grid) =>
			hasText(grid, options.surface === 'quota' ? 'Quota status' : 'Codex Alpha'),
		);
		if (options.plan.pickerOnly && options.surface === 'sessions-filters') {
			// The component fixture's first draw measures ScrollList; draw 2 paints
			// those measurements. Bind to received completed frames, even if both
			// arrived in one PTY chunk. Never ask for an arbitrary *next* frame.
			await driver.waitForGrid(
				'main after initial measurement',
				(grid) => hasText(grid, 'Codex Alpha'),
				1,
			);
			const serializedFrames = driver.bytes().toString('latin1').split('\x1b[?2026l').length - 1;
			if (serializedFrames !== driver.frameGeneration())
				throw new Error('received frame ordinal differs from terminal parser generation');
			writeFileSync(
				join(directory, 'entry-frame-binding.json'),
				JSON.stringify(
					{
						kind: 'post-measurement-entry',
						minimumCompletedFrame: 2,
						receivedCompletedFrame: serializedFrames,
						sourceHead: options.plan.productSourceHead,
						product: options.plan.product,
						driver: options.plan.driver,
					},
					null,
					2,
				),
			);
		}
		saveCheckpoint(driver, directory, 'main');
		return await options.observe(driver, directory);
	} finally {
		await driver.stop();
	}
}
function fixedKeyAcknowledgement(
	name: string,
	surface: InputConsumptionExpectation['surface'],
	route: { readonly action: string | null; readonly target?: string },
): InputConsumptionExpectation {
	return {
		event: { kind: 'key', code: { kind: 'fixed', name }, modifiers: [], phase: 'press' },
		surface,
		target: route.target ?? null,
		action: route.action,
	};
}
// TargetId::child prefixes the parent's UTF-8 byte length, not its character count.
function childTarget(parentTarget: string, suffix: string): string {
	return `${Buffer.byteLength(parentTarget, 'utf8')}:${parentTarget}:${suffix}`;
}
export async function observeMainEscape(
	plan: BaselinePlan,
	keys: KeyProtocol,
	surface: 'sessions-filters' | 'quota',
): Promise<BaselineResult> {
	return withTerminal({
		plan,
		keys,
		surface,
		name: 'main-escape',
		sessionsReceipts: surface === 'sessions-filters',
		observe: async (driver, directory) => {
			const receipts =
				surface === 'sessions-filters'
					? InputConsumptionReadback.open(
							join(directory, 'session-receipts', 'input-dispatch.jsonl'),
						)
					: null;
			const initial = gridLines(driver.grid());
			const helpActionId = 'help';
			const title = surface === 'quota' ? 'Quota Help' : 'Help';
			const generation = driver.frameGeneration();
			// Help is a real processing barrier after the ignored Esc; neither
			// a stale grid nor a live PID establishes that Esc was handled.
			if (receipts === null) {
				// Quota's producer is a separate S7 prerequisite; this is its retained baseline.
				await driver.send(Buffer.concat([keyBytes(keys, 'escape'), keyBytes(keys, 'help')]));
			} else {
				// Legacy 1b 1b 4f 50 is ambiguous with Alt+F1. A decoded Esc
				// acknowledgement, not a delay or a transport write, separates them.
				await receipts.observeInput({
					expected: [fixedKeyAcknowledgement('Esc', { kind: 'screen' }, { action: 'escape' })],
					inject: async () => {
						const sequence = receipts.sequence + 1;
						await driver.send(keyBytes(keys, 'escape'));
						await driver.waitForDispatchFrame(sequence);
					},
					timeoutMs: 8_000,
				});
				await receipts.observeInput({
					expected: [fixedKeyAcknowledgement('F1', { kind: 'screen' }, { action: helpActionId })],
					inject: async () => {
						const sequence = receipts.sequence + 1;
						await driver.send(keyBytes(keys, 'help'));
						await driver.waitForDispatchFrame(sequence);
					},
					timeoutMs: 8_000,
				});
			}
			await driver.waitForGrid(
				'Help after main Esc',
				(grid) => findDialog(grid, title) !== null,
				generation,
			);
			const opened = driver.frameGeneration();
			if (receipts === null) await driver.send(keyBytes(keys, 'escape'));
			else
				await receipts.observeInput({
					expected: [
						fixedKeyAcknowledgement(
							'Esc',
							{ kind: 'overlay', overlayId: '1' },
							{ action: null, target: childTarget(`action:${helpActionId}`, 'help-body') },
						),
					],
					inject: async () => {
						const sequence = receipts.sequence + 1;
						await driver.send(keyBytes(keys, 'escape'));
						await driver.waitForDispatchFrame(sequence);
					},
					timeoutMs: 8_000,
				});
			await driver.waitForGrid(
				'main restored after Help',
				(grid) => findDialog(grid, title) === null,
				opened,
			);
			const unchanged = JSON.stringify(gridLines(driver.grid())) === JSON.stringify(initial);
			const running = await driver.running();
			saveCheckpoint(driver, directory, 'after-escape');
			const quit = (): Promise<void> =>
				driver.send(Buffer.from(keys === 'legacy' ? '\x03' : '\x1b[99;5u'));
			let exitCode: number | null = null;
			if (receipts === null) await quit();
			else
				await receipts.observeInput({
					expected: [
						{
							event: {
								kind: 'key',
								code: { kind: 'shortcut', key: 'c' },
								modifiers: ['control'],
								phase: 'press',
							},
							surface: { kind: 'screen' },
							target: null,
							action: 'quit',
						},
					],
					inject: async () => {
						await quit();
						exitCode = await driver.drainUntilExit();
					},
					timeoutMs: 8_000,
				});
			if (receipts === null) exitCode = await driver.drainUntilExit();
			const transcript = driver.bytes().toString('utf8');
			const exited = exitCode === 0;
			receipts?.assertComplete();
			const outcomes = transcript.match(/SESSION_PICKER_OUTCOME [^\r\n]*/gu) ?? [];
			const cancelled =
				surface === 'quota' ||
				(outcomes.length === 1 && outcomes[0] === 'SESSION_PICKER_OUTCOME canceled');
			return {
				name: `${surface} Esc never quits`,
				keys,
				kind: 'known-defect',
				result: running && unchanged && exited && cancelled ? 'pass' : 'red',
				expected: 'main remains running at default filters',
				observed: {
					running,
					unchanged,
					ctrlCExit: exited,
					inputAcknowledgements: receipts?.sequence ?? null,
					pickerOutcome: transcript.includes('SESSION_PICKER_OUTCOME canceled'),
					terminalRestored: transcript.includes('\x1b[?1049l'),
				},
			};
		},
	});
}
async function observeNewFocus(plan: BaselinePlan, keys: KeyProtocol): Promise<BaselineResult> {
	return withTerminal({
		plan,
		keys,
		surface: 'sessions-filters',
		name: 'new-focus',
		observe: async (driver, directory) => {
			const first = driver.frameGeneration();
			await driver.send(keyBytes(keys, 'new'));
			await driver.waitForGrid('New session', (grid) => hasText(grid, 'New session'), first);
			const initial = focusedWords(driver.grid(), 'New session');
			saveCheckpoint(driver, directory, 'new-open');
			const before = driver.frameGeneration();
			// Tab is an observable barrier after Down: the wrong current build goes
			// Start -> (Down ignored) -> Client; the adopted field traversal differs.
			await driver.send(Buffer.concat([keyBytes(keys, 'down'), keyBytes(keys, 'tab')]));
			await driver.waitForGrid(
				'Down then Tab processed',
				(grid) =>
					hasText(grid, 'New session') && !focusedWords(grid, 'New session').includes('Start'),
				before,
			);
			const after = focusedWords(driver.grid(), 'New session');
			saveCheckpoint(driver, directory, 'new-down-tab');
			return {
				name: 'New focus starts on a field; Down moves fields',
				keys,
				kind: 'known-defect',
				result: initial.includes('Start') ? 'red' : 'pass',
				expected: 'first enabled field is focused and Down advances to the next field',
				observed: { initial, afterDownThenTab: after },
			};
		},
	});
}
async function observeQuotaRhythm(plan: BaselinePlan, keys: KeyProtocol): Promise<BaselineResult> {
	return withTerminal({
		plan,
		keys,
		surface: 'quota',
		name: 'details-rhythm',
		observe: async (driver, directory) => {
			const before = driver.frameGeneration();
			await driver.send(keyBytes(keys, 'details'));
			await driver.waitForGrid(
				'Account details',
				(grid) => hasText(grid, 'Account details'),
				before,
			);
			const grid = driver.grid();
			const rect = findDialog(grid, 'Account details');
			if (rect === null) throw new Error('Account details frame unreachable');
			const heading = gridLines(grid).findIndex((line) => line.includes('Quota windows'));
			if (heading < 1) throw new Error('Quota windows not visible at required viewport');
			const previous = grid.cells[heading - 1]
				?.slice(rect.left + 1, rect.right)
				.map(cellText)
				.join('')
				.trim();
			saveCheckpoint(driver, directory, 'account-details');
			return {
				name: 'one blank row before Quota windows',
				keys,
				kind: 'known-defect',
				result: previous === '' ? 'pass' : 'red',
				expected: 'exactly one blank row before Quota windows section',
				observed: { heading, precedingRow: previous, rect },
			};
		},
	});
}
export async function observeIsolation(
	plan: BaselinePlan,
	keys: KeyProtocol,
	surface: 'sessions-filters' | 'quota',
	title: string,
	open: Uint8Array,
): Promise<BaselineResult[]> {
	return withTerminal({
		plan,
		keys,
		surface,
		name: `isolation-${title.replaceAll(' ', '-')}`,
		observe: async (driver, directory) => {
			const base = driver.grid();
			const before = driver.frameGeneration();
			const input = Buffer.from('abc\x1b[200~literal-id-paste\x1b[201~');
			await driver.send(Buffer.concat([open, input, keyBytes(keys, 'down')]));
			await driver.waitForGrid(title, (grid) => findDialog(grid, title) !== null, before);
			const covered = driver.grid();
			const rect = findDialog(covered, title);
			if (rect === null) throw new Error(`overlay entry missing: ${title}`);
			saveCheckpoint(driver, directory, 'overlay-open');
			const unchangedVisible = base.cells.every((row, rowIndex) =>
				row.every(
					(cell, column) =>
						(rowIndex >= rect.top &&
							rowIndex <= rect.bottom &&
							column >= rect.left - 1 &&
							column <= rect.right) ||
						cellText(cell) === cellText(covered.cells[rowIndex]?.[column] ?? cell),
				),
			);
			const exposed = base.cells
				.flatMap((row, rowIndex) => row.map((cell, column) => ({ cell, column, rowIndex })))
				.filter(
					({ cell, column, rowIndex }) =>
						cellText(cell).trim() !== '' &&
						!(
							rowIndex >= rect.top &&
							rowIndex <= rect.bottom &&
							column >= rect.left - 1 &&
							column <= rect.right
						),
				);
			const dimmed =
				exposed.length > 0 &&
				exposed.every(({ cell, column, rowIndex }) => {
					const next = covered.cells[rowIndex]?.[column];
					return (
						next !== undefined &&
						(next.fgRgb !== cell.fgRgb || next.flags !== cell.flags || next.bgRgb !== cell.bgRgb)
					);
				});
			let helpLegend: string | undefined;
			if (surface === 'sessions-filters' && title === 'Help') {
				const helpText = covered.cells
					.slice(rect.top, rect.bottom + 1)
					.map((row) =>
						row
							.slice(rect.left, rect.right + 1)
							.map(cellText)
							.join(''),
					)
					.join('\n');
				if (!helpText.includes('Screen') || !helpText.includes('Sessions'))
					throw new Error('shared Help lacks its Sessions context');
				if (!hasText(driver.grid(), 'never quit')) {
					const beforeScroll = driver.frameGeneration();
					await driver.send(keyBytes(keys, 'end'));
					await driver.waitForGrid(
						'Esc legend visible',
						(grid) => hasText(grid, 'never quit'),
						beforeScroll,
					);
				}
				helpLegend = gridLines(driver.grid()).find((line) => line.includes('never quit'));
				saveCheckpoint(driver, directory, 'help-escape-legend');
			}
			const close = driver.frameGeneration();
			// The outside click and close share one transport write. Closing supplies
			// an observable barrier after an otherwise ignored click.
			await driver.send(Buffer.concat([Buffer.from('\x1b[<0;6;10M'), keyBytes(keys, 'escape')]));
			await driver.waitForGrid('overlay closed', (grid) => findDialog(grid, title) === null, close);
			const restored = driver.grid();
			saveCheckpoint(driver, directory, 'overlay-closed');
			const sameInput = gridLines(base).join('\n') === gridLines(restored).join('\n');
			const observations: BaselineResult[] = [
				{
					name: `${surface} ${title} input isolation`,
					keys,
					kind: 'isolation',
					result: sameInput ? 'pass' : 'red',
					expected:
						'text/paste/arrows in one PTY write leave underlying text and selection unchanged',
					// Covered presentation can change (R1). A hidden footer is
					// a U2 drawing failure, not evidence of changed input state.
					observed: {
						unchangedVisible,
						sameInput,
						visibleChangesArePresentationOnly: sameInput && !unchangedVisible,
					},
				},
				{
					name: `${surface} ${title} dimmed`,
					keys,
					kind: 'isolation',
					result: dimmed ? 'pass' : 'red',
					expected: 'all exposed underlying text uses its covered role',
					observed: { dimmed, exposedCells: exposed.length },
				},
			];
			if (surface === 'sessions-filters' && title === 'Help') {
				if (helpLegend === undefined) throw new Error('named Esc Help legend not reached');
				observations.push({
					name: 'Help describes Esc as never quitting',
					keys,
					kind: 'known-defect',
					result: 'pass',
					expected: 'Help names the actual main-screen Esc behaviour',
					observed: {
						legend: helpLegend.trim(),
						actualAtDefaults: 'checked separately through Help barrier and Ctrl+C outcome',
					},
				});
			}
			return observations;
		},
	});
}
export async function runBaseline(plan: BaselinePlan): Promise<readonly BaselineResult[]> {
	mkdirSync(plan.output);
	writeFileSync(join(plan.output, 'target.json'), JSON.stringify(plan, null, 2));
	const results: BaselineResult[] = [];
	const runMode = async (keys: KeyProtocol): Promise<void> => {
		results.push(await observeMainEscape(plan, keys, 'sessions-filters'));
		if (!plan.pickerOnly) results.push(await observeMainEscape(plan, keys, 'quota'));
		results.push(await observeNewFocus(plan, keys));
		if (!plan.pickerOnly) results.push(await observeQuotaRhythm(plan, keys));
		results.push(
			...(await observeIsolation(plan, keys, 'sessions-filters', 'Help', keyBytes(keys, 'help'))),
		);
		results.push(
			...(await observeIsolation(
				plan,
				keys,
				'sessions-filters',
				'New session',
				keyBytes(keys, 'new'),
			)),
		);
		results.push(
			...(await observeIsolation(
				plan,
				keys,
				'sessions-filters',
				'Session details',
				keyBytes(keys, 'details'),
			)),
		);
		results.push(
			...(await observeIsolation(
				plan,
				keys,
				'sessions-filters',
				'Session filters',
				Buffer.from(keys === 'legacy' ? '\x06' : '\x1b[102;5u'),
			)),
		);
		if (!plan.pickerOnly)
			results.push(
				...(await observeIsolation(plan, keys, 'quota', 'Quota Help', keyBytes(keys, 'help'))),
			);
		if (!plan.pickerOnly)
			results.push(
				...(await observeIsolation(
					plan,
					keys,
					'quota',
					'Account details',
					keyBytes(keys, 'details'),
				)),
			);
		if (!plan.pickerOnly)
			results.push(
				...(await observeIsolation(plan, keys, 'quota', 'Pool details', Buffer.from('\x1b[15~'))),
			);
		for (const entry of ['≡ menu and toolbar', 'ComboBox dropdown'])
			results.push({
				name: entry,
				keys,
				kind: 'new-entry',
				result: 'absent-at-base',
				expected: 'introduced by S6/S7',
				observed: 'no current entry',
			});
		results.push({
			name: 'wide glyph on overlay-left−1',
			keys,
			kind: 'new-entry',
			result: 'entry-pending-fixture',
			expected: 'U9 left-border baseline',
			observed: 'Implementer owns selectable seed',
		});
	};
	try {
		await runMode('legacy');
		await runMode('kitty');
		return results;
	} finally {
		writeFileSync(join(plan.output, 'baseline-results.json'), JSON.stringify(results, null, 2));
	}
}
export async function checkBaselineTarget(plan: BaselinePlan): Promise<void> {
	for (const artifact of [plan.driver, plan.product]) {
		if (createHash('sha256').update(readFileSync(artifact.path)).digest('hex') !== artifact.sha256)
			throw new Error(`binary changed: ${artifact.path}`);
		execFileSync('/usr/bin/codesign', ['--verify', '--strict', artifact.path]);
	}
	const head = execFileSync('git', ['rev-parse', 'HEAD'], {
		cwd: plan.repository,
		encoding: 'utf8',
	}).trim();
	if (head !== plan.productSourceHead) throw new Error(`baseline source changed: ${head}`);
	await qualifyTerminal();
}
if (isMain(import.meta.url)) {
	if (process.argv[2] === '--help')
		process.stdout.write(
			'baseline-journeys.ts PLAN.json [--check]: qualified wterm grids through the signed Rust PTY driver at the declared pre-UI build. Captures do not prove physical keyboard input.\n',
		);
	else {
		const plan = baselinePlanSchema.parse(JSON.parse(readFileSync(process.argv[2] ?? '', 'utf8')));
		await checkBaselineTarget(plan)
			.then(async () => {
				if (process.argv.includes('--check'))
					process.stdout.write('baseline target and emulator qualified; no product journey run\n');
				else {
					const results = await runBaseline(plan);
					process.stdout.write(`${results.length} baseline results in ${plan.output}\n`);
				}
			})
			.catch(reportCliFailure);
	}
}
