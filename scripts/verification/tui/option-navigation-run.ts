/** Run source-pinned picker/Help aliases after S6b declaration and Lead's native window.
 * node scripts/verification/tui/option-navigation-run.ts PLAN.json [--check]
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, isAbsolute } from 'node:path';

import { isMain, planArgument, reportCliFailure, strictArguments } from './cli-arguments.ts';
import { waitForMeasuredDialog } from './dialog-frame-readiness.ts';
import type { ProcessIdentity } from './evidence-schema.ts';
import type { GhosttyKey } from './ghostty-input-encoding.ts';
import { GhosttyJourneyInput, requireDelivered } from './ghostty-journey-input.ts';
import { findDialog } from './grid-scene-matcher.ts';
import { InputConsumptionReadback } from './input-consumption-readback.ts';
import type {
	InputConsumptionExpectation,
	InputDispatchReceipt,
} from './input-consumption-schema.ts';
import {
	appendOneHelpRow,
	helpViewport,
	pickerPageRows,
	requireHelpRows,
	selectedFixtureRow,
} from './option-navigation-observation.ts';
import { optionNavigationPlanSchema } from './option-navigation-plan.ts';
import type { OptionNavigationPlan } from './option-navigation-plan.ts';
import { lookupProcessIdentity, sameIdentity } from './process-identity.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { KeyProtocol } from './pty-grid-driver.ts';
import { canonicalJson, sourceBinding, verifyArtifact } from './source-binding.ts';
import { gridLines } from './terminal-grid.ts';
import { renderOriginal } from './terminal-original.ts';
import { qualifyTerminal } from './terminal-qualification.ts';

class NavigationMismatch extends Error {}
class CleanupUnsettled extends Error {}
const helpTarget = '11:action:help:help-body';
function expectedKey(key: GhosttyKey, help: boolean): InputConsumptionExpectation {
	const direction = key.endsWith('Up') ? 'Up' : key.endsWith('Down') ? 'Down' : null;
	const event: InputConsumptionExpectation['event'] =
		direction !== null
			? {
					kind: 'key',
					code: { kind: 'fixed', name: direction },
					modifiers: key.includes('Shift')
						? ['shift', 'alt']
						: key.startsWith('Alt')
							? ['alt']
							: [],
					phase: 'press',
				}
			: {
					kind: 'key',
					code: { kind: 'fixed', name: key === 'Escape' ? 'Esc' : key === 'Ctrl+C' ? 'c' : key },
					modifiers: key === 'Ctrl+C' ? ['control'] : [],
					phase: 'press',
				};
	if (key === 'Ctrl+C')
		return {
			event: {
				kind: 'key',
				code: { kind: 'shortcut', key: 'c' },
				modifiers: ['control'],
				phase: 'press',
			},
			surface: { kind: 'screen' },
			target: null,
			action: 'quit',
		};
	return {
		event,
		surface: help ? { kind: 'overlay', overlayId: '1' } : { kind: 'screen' },
		target: key === 'F1' ? null : help ? helpTarget : 'sessions-list',
		action: key === 'F1' ? 'help' : null,
	};
}
export async function checkOptionNavigationTarget(plan: OptionNavigationPlan): Promise<void> {
	if (!isAbsolute(plan.output) || !isAbsolute(plan.repository))
		throw new Error('absolute target/output paths required');
	if (canonicalJson(sourceBinding(plan.repository)) !== canonicalJson(plan.source))
		throw new Error('declared S6b source snapshot changed');
	if (
		canonicalJson(sourceBinding(plan.observer.repository)) !== canonicalJson(plan.observer.source)
	)
		throw new Error('declared observer source changed');
	execFileSync('git', [
		'-C',
		plan.repository,
		'merge-base',
		'--is-ancestor',
		plan.requiredCrateHead,
		plan.source.head,
	]);
	if (process.version !== 'v26.12.1' || realpathSync(process.execPath) !== plan.node.path)
		throw new Error('pinned Node required');
	for (const artifact of [plan.driver, plan.product, plan.node]) verifyArtifact(artifact);
	for (const artifact of [plan.driver, plan.product])
		execFileSync('/usr/bin/codesign', ['--verify', '--strict', artifact.path]);
	const listed = execFileSync(
		plan.driver.path,
		['compiled_grid_terminal_observation', '--exact', '--list'],
		{ encoding: 'utf8' },
	);
	if (!listed.includes('compiled_grid_terminal_observation: test'))
		throw new Error('compiled PTY entry absent');
	await qualifyTerminal();
}
async function observeCase(
	plan: OptionNavigationPlan,
	keys: KeyProtocol,
	dimensions: string,
): Promise<void> {
	const directory = join(plan.output, `${keys}-${dimensions}`);
	mkdirSync(directory);
	const receiptsDirectory = join(directory, 'session-receipts');
	mkdirSync(receiptsDirectory);
	const driver = await PtyGridDriver.start({
		node: plan.node,
		repository: plan.repository,
		driver: plan.driver.path,
		surface: 'sessions-filters',
		keys,
		evidence: directory,
		sessionsReceiptDirectory: receiptsDirectory,
	});
	const input = new GhosttyJourneyInput(driver, 'example-ghostty-keybinds');
	let reader: InputConsumptionReadback | null = null;
	let ownedIdentity: ProcessIdentity | null = null;
	let failure: { kind: 'none' } | { kind: 'failed'; error: unknown } = { kind: 'none' };
	let aliveAfterStop = false;
	const actions: unknown[] = [];
	try {
		await driver.waitForGrid(
			'picker initial measured entry',
			(grid) => {
				try {
					return selectedFixtureRow(grid, plan.rows) === 0;
				} catch {
					return false;
				}
			},
			1,
		);
		const pid = Number(readFileSync(join(directory, 'quota-child.pid'), 'utf8').trim());
		const identity = lookupProcessIdentity(pid);
		if (
			identity === null ||
			identity.realUid !== process.getuid?.() ||
			realpathSync(identity.executable) !== plan.product.path
		)
			throw new Error('declared picker process binding mismatch');
		ownedIdentity = identity;
		writeFileSync(
			join(directory, 'child-binding.json'),
			JSON.stringify(
				{ identity, product: plan.product, driver: plan.driver, source: plan.source },
				null,
				2,
			),
		);
		reader = InputConsumptionReadback.open(join(receiptsDirectory, 'input-dispatch.jsonl'));
		const observeInput = async (
			expected: InputConsumptionExpectation,
			inject: () => Promise<void>,
		): Promise<InputDispatchReceipt> => {
			if (reader === null) throw new Error('receipt reader absent');
			const [ack] = await reader.observeInput({ expected: [expected], inject, timeoutMs: 8000 });
			if (ack === undefined) throw new Error('dispatch acknowledgement absent');
			actions.push(ack);
			return ack;
		};
		const capture = async (name: string): Promise<void> => {
			writeFileSync(join(directory, name + '.ansi'), driver.bytes());
			writeFileSync(join(directory, name + '.grid.json'), JSON.stringify(driver.grid(), null, 2));
			writeFileSync(join(directory, name + '.txt'), gridLines(driver.grid()).join('\n') + '\n');
			await renderOriginal({
				chrome: plan.chrome,
				output: directory,
				bytes: driver.bytes(),
				grid: driver.grid(),
				resizes: driver.resizeHistory(),
				columns: driver.grid().columns,
				rows: driver.grid().rows,
				checkpoint: name,
				sourceHead: plan.source.head,
				binarySha256: plan.product.sha256,
			});
		};
		const [columns, rows] = dimensions.split('x').map(Number);
		if (columns === undefined || rows === undefined) throw new Error('dimensions invalid');
		if (columns !== 120 || rows !== 36) {
			const before = driver.frameGeneration();
			await observeInput(
				{
					event: { kind: 'resize', columns, rows },
					surface: { kind: 'screen' },
					target: null,
					action: null,
				},
				() => driver.resize(columns, rows),
			);
			await driver.waitForGrid(
				'same-child resized picker',
				(grid) => {
					try {
						return selectedFixtureRow(grid, plan.rows) === 0;
					} catch {
						return false;
					}
				},
				before,
			);
		}
		const resizedIdentity = lookupProcessIdentity(pid);
		if (
			resizedIdentity === null ||
			!sameIdentity(identity, resizedIdentity) ||
			!(await driver.running())
		)
			throw new Error('picker identity changed after resize');
		for (const key of ['PageUp', 'PageDown', 'Home', 'End'])
			if ((await input.key(key)).delivery.kind !== 'consumed')
				throw new Error('original key not consumed by nominated model');
		await capture('entry');
		const page = pickerPageRows(driver.grid());
		const last = plan.rows.length - 1;
		const listStep = async (key: GhosttyKey, index: number, name: string): Promise<void> => {
			const before = driver.frameGeneration();
			const previous = selectedFixtureRow(driver.grid(), plan.rows);
			await observeInput(expectedKey(key, false), async () => {
				requireDelivered(await input.key(key));
			});
			if (previous !== index)
				await driver.waitForGrid(
					name,
					(grid) => {
						try {
							selectedFixtureRow(grid, plan.rows);
							return true;
						} catch {
							return false;
						}
					},
					before,
				);
			const actual = selectedFixtureRow(driver.grid(), plan.rows);
			if (actual !== index)
				throw new NavigationMismatch(`${name}: expected fixture row ${index}, observed ${actual}`);
			actions.push({ name, page, expectedIndex: index, observedIndex: actual });
			await capture(name);
		};
		await listStep('Alt+Shift+Down', last, 'list-last');
		await listStep('Alt+Up', Math.max(0, last - page), 'list-page-up');
		await listStep('Alt+Shift+Up', 0, 'list-first');
		await listStep('Alt+Down', Math.min(last, page), 'list-page-down');
		const selectedBeforeHelp = selectedFixtureRow(driver.grid(), plan.rows);
		const beforeHelp = driver.frameGeneration();
		const openAck = await observeInput(expectedKey('F1', false), async () => {
			requireDelivered(await input.key('F1'));
		});
		await waitForMeasuredDialog({
			driver,
			directory,
			title: 'Help',
			checkpoint: 'help',
			before: beforeHelp,
			acknowledgement: openAck,
		});
		const initial = helpViewport(driver.grid());
		if (!initial.rows.join(' ').includes('Context'))
			throw new Error('Help did not start at its first content');
		const full = [...initial.rows];
		const collectRows = async (remaining: number): Promise<void> => {
			const before = helpViewport(driver.grid());
			if (before.atEnd) return;
			if (remaining === 0) throw new Error('bounded Help row qualification exhausted');
			const frame = driver.frameGeneration();
			await observeInput(expectedKey('Down', true), async () => {
				requireDelivered(await input.key('Down'));
			});
			await driver.waitForGrid('one actual Help row', () => true, frame);
			full.push(appendOneHelpRow(before.rows, helpViewport(driver.grid()).rows));
			await collectRows(remaining - 1);
		};
		await collectRows(2048);
		if (!full.join(' ').replace(/\s+/gu, ' ').includes('never quit'))
			throw new Error('Help terminal convention absent from full physical content');
		writeFileSync(
			join(directory, 'help-physical-row-oracle.json'),
			JSON.stringify(
				{
					basis:
						'observed physical one-row Down transitions; independently checked overlap, not app scroll counters',
					viewportRows: initial.rows.length,
					full,
				},
				null,
				2,
			),
		);
		const maximum = full.length - initial.rows.length;
		if (maximum <= 0) throw new Error('Help fixture cannot demonstrate paging');
		const helpStep = async (key: GhosttyKey, offset: number, name: string): Promise<void> => {
			const expected = full.slice(offset, offset + initial.rows.length);
			const previous = helpViewport(driver.grid()).rows;
			const frame = driver.frameGeneration();
			await observeInput(expectedKey(key, true), async () => {
				requireDelivered(await input.key(key));
			});
			if (JSON.stringify(previous) !== JSON.stringify(expected))
				await driver.waitForGrid(name, () => true, frame);
			try {
				requireHelpRows(helpViewport(driver.grid()).rows, expected);
			} catch (error) {
				throw new NavigationMismatch(`${name}: ${String(error)}`);
			}
			actions.push({ name, offset, viewportRows: initial.rows.length });
			await capture(name);
		};
		await helpStep('Alt+Shift+Up', 0, 'help-first');
		await helpStep('Alt+Down', Math.min(maximum, initial.rows.length), 'help-page-down');
		await helpStep('Alt+Shift+Down', maximum, 'help-last');
		await helpStep('Alt+Up', Math.max(0, maximum - initial.rows.length), 'help-page-up');
		const closeFrame = driver.frameGeneration();
		await observeInput(expectedKey('Escape', true), async () => {
			requireDelivered(await input.key('Escape'));
		});
		await driver.waitForGrid(
			'return from Help',
			(grid) => findDialog(grid, 'Help') === null,
			closeFrame,
		);
		if (selectedFixtureRow(driver.grid(), plan.rows) !== selectedBeforeHelp)
			throw new NavigationMismatch('Help changed the covered picker selection');
		await capture('help-return');
		await observeInput(expectedKey('Ctrl+C', false), async () => {
			requireDelivered(await input.key('Ctrl+C'));
		});
		if ((await driver.drainUntilExit()) !== 0) throw new Error('picker did not exit cleanly');
		reader.assertComplete();
	} catch (error) {
		failure = { kind: 'failed', error };
	} finally {
		await driver.stop();
		writeFileSync(join(directory, 'input-delivery.json'), JSON.stringify(input.history(), null, 2));
		writeFileSync(join(directory, 'observations.json'), JSON.stringify(actions, null, 2));
		const live = ownedIdentity === null ? null : lookupProcessIdentity(ownedIdentity.pid);
		const alive = ownedIdentity !== null && live !== null && sameIdentity(live, ownedIdentity);
		aliveAfterStop = alive;
		writeFileSync(
			join(directory, 'cleanup.json'),
			JSON.stringify({ identity: ownedIdentity, sameOwnedChildAlive: alive }, null, 2),
		);
	}
	if (aliveAfterStop)
		throw new CleanupUnsettled(
			`owned picker remains alive; stop allocating cases; primary outcome: ${failure.kind === 'failed' ? String(failure.error) : 'completed'}`,
		);
	if (failure.kind === 'failed') throw failure.error;
}
export async function runOptionNavigation(value: unknown, checkOnly = false): Promise<void> {
	const plan = optionNavigationPlanSchema.parse(value);
	await checkOptionNavigationTarget(plan);
	if (checkOnly) return;
	mkdirSync(plan.output);
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	const results: {
		keys: KeyProtocol;
		grid: string;
		status: 'pass' | 'fail' | 'unobserved';
		reason: string | null;
	}[] = [];
	await plan.keys.reduce<Promise<void>>(async (previous, keys) => {
		await previous;
		await plan.grids.reduce<Promise<void>>(async (prior, grid) => {
			await prior;
			await checkOptionNavigationTarget(plan);
			let status: 'pass' | 'fail' | 'unobserved' = 'pass';
			let reason: string | null = null;
			try {
				await observeCase(plan, keys, grid);
			} catch (error) {
				if (error instanceof CleanupUnsettled) throw error;
				status =
					error instanceof NavigationMismatch ||
					/input acknowledgement .* mismatch/u.test(String(error))
						? 'fail'
						: 'unobserved';
				reason = String(error);
			}
			results.push({ keys, grid, status, reason });
			writeFileSync(
				join(plan.output, 'results.json'),
				JSON.stringify({ source: plan.source.head, results }, null, 2),
			);
			process.stdout.write(`${keys} ${grid} ${status}${reason === null ? '' : ': ' + reason}\n`);
		}, Promise.resolve());
	}, Promise.resolve());
	await checkOptionNavigationTarget(plan);
	if (results.some((row) => row.status !== 'pass'))
		throw new Error('Option navigation includes contrary or unobserved cases');
}
if (isMain(import.meta.url)) {
	try {
		const { values, positionals } = strictArguments(
			process.argv.slice(2),
			{ check: { type: 'boolean' } },
			true,
		);
		await runOptionNavigation(
			JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
			values.check === true,
		);
	} catch (error) {
		reportCliFailure(error);
	}
}
