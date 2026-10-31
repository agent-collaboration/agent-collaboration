/** Run source-pinned picker/Help aliases after S6b declaration and Lead's native window.
 * node scripts/verification/tui/capture/option-navigation-run.ts PLAN.json [--check]
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, isAbsolute } from 'node:path';

import { isMain, planArgument, reportCliFailure, strictArguments } from '../cli/cli-arguments.ts';
import type { ProcessIdentity } from '../evidence/evidence-schema.ts';
import {
	canonicalJson,
	sourceBinding,
	sourceDigest,
	verifyArtifact,
} from '../evidence/source-binding.ts';
import { findDialog } from '../journeys/scenes/grid-scene-matcher.ts';
import { optionPickerHelpCensus } from '../journeys/scenes/option-help-census.ts';
import {
	appendOneHelpRow,
	helpViewport,
	helpLayout,
	requireFullHelpCensus,
	requireHelpNoop,
	pickerPageRows,
	requireHelpRows,
	selectedFixtureRow,
} from '../journeys/scenes/option-navigation-observation.ts';
import { lookupProcessIdentity, sameIdentity } from '../processes/process-identity.ts';
import { InputConsumptionReadback } from '../sessions/input-consumption-readback.ts';
import type {
	InputConsumptionExpectation,
	InputDispatchReceipt,
} from '../sessions/input-consumption-schema.ts';
import { readSourceQualifiedOutcome } from '../sessions/source-qualified-outcome.ts';
import type { GhosttyKey } from '../terminal/ghostty-input-encoding.ts';
import { GhosttyJourneyInput, requireDelivered } from '../terminal/ghostty-journey-input.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import type { KeyProtocol } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import { renderOriginal } from '../terminal/terminal-original.ts';
import { qualifyTerminal } from '../terminal/terminal-qualification.ts';
import { optionNavigationPlanSchema } from './option-navigation-plan.ts';
import type { OptionNavigationPlan } from './option-navigation-plan.ts';

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
		canonicalJson(sourceBinding(plan.driverSource.repository)) !==
			canonicalJson(plan.driverSource.source) ||
		sourceDigest(plan.driverSource.source) !== plan.driverSourceDigest ||
		sourceDigest(plan.source) !== plan.productSourceDigest
	)
		throw new Error('declared driver/product source digests changed');
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
): Promise<'fit' | 'overflow'> {
	const directory = join(plan.output, `${keys}-${dimensions}`);
	mkdirSync(directory);
	const receiptsDirectory = join(directory, 'session-receipts');
	mkdirSync(receiptsDirectory);
	const driver = await PtyGridDriver.start({
		node: plan.node,
		product: plan.product,
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
	let observedHelpLayout: 'fit' | 'overflow' | null = null;
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
		// This entry spawns the product directly through portable-pty; PID files
		// belong to the separate public-wrapper entry and are not produced here.
		const driverPid = driver.processId();
		const children = execFileSync('/bin/ps', ['-axo', 'pid=,ppid='], { encoding: 'utf8' })
			.trim()
			.split('\n')
			.flatMap((line) => {
				const [childPid, parentPid] = line.trim().split(/\s+/u).map(Number);
				if (parentPid !== driverPid || childPid === undefined) return [];
				const child = lookupProcessIdentity(childPid);
				return child !== null &&
					child.realUid === process.getuid?.() &&
					realpathSync(child.executable) === plan.product.path
					? [child]
					: [];
			});
		if (children.length !== 1) throw new Error('one exact owned driver product child required');
		const identity = children[0];
		if (
			identity === undefined ||
			identity.realUid !== process.getuid?.() ||
			realpathSync(identity.executable) !== plan.product.path
		)
			throw new Error('declared picker process binding mismatch');
		const pid = identity.pid;
		ownedIdentity = identity;
		writeFileSync(
			join(directory, 'child-binding.json'),
			JSON.stringify(
				{
					identity,
					parentDriverPid: driverPid,
					product: plan.product,
					driver: plan.driver,
					source: plan.source,
				},
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
		let current = { grid: driver.grid(), bytes: driver.bytes(), resizes: driver.resizeHistory() };
		const mark = async (ack: InputDispatchReceipt): Promise<void> => {
			current = await driver.waitForDispatchFrame(ack.sequence);
		};
		const capture = async (name: string): Promise<void> => {
			writeFileSync(join(directory, name + '.ansi'), current.bytes);
			writeFileSync(join(directory, name + '.grid.json'), JSON.stringify(current.grid, null, 2));
			writeFileSync(join(directory, name + '.txt'), gridLines(current.grid).join('\n') + '\n');
			await renderOriginal({
				chrome: plan.chrome,
				output: directory,
				bytes: current.bytes,
				grid: current.grid,
				resizes: current.resizes,
				columns: current.grid.columns,
				rows: current.grid.rows,
				checkpoint: name,
				sourceHead: plan.source.head,
				binarySha256: plan.product.sha256,
			});
		};
		const [columns, rows] = dimensions.split('x').map(Number);
		if (columns === undefined || rows === undefined) throw new Error('dimensions invalid');
		if (columns !== 120 || rows !== 36) {
			const resized = await observeInput(
				{
					event: { kind: 'resize', columns, rows },
					surface: { kind: 'screen' },
					target: null,
					action: null,
				},
				() => driver.resize(columns, rows),
			);
			await mark(resized);
			if (current.grid.columns !== columns || current.grid.rows !== rows)
				throw new Error('marked resize frame dimensions differ');
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
		const page = pickerPageRows(current.grid);
		const last = plan.rows.length - 1;
		const listStep = async (key: GhosttyKey, index: number, name: string): Promise<void> => {
			const ack = await observeInput(expectedKey(key, false), async () => {
				requireDelivered(await input.key(key));
			});
			await mark(ack);
			const actual = selectedFixtureRow(current.grid, plan.rows);
			if (actual !== index)
				throw new NavigationMismatch(`${name}: expected fixture row ${index}, observed ${actual}`);
			actions.push({ name, page, expectedIndex: index, observedIndex: actual });
			await capture(name);
		};
		await listStep('Alt+Shift+Up', 0, 'list-first-noop');
		await listStep('Alt+Shift+Down', last, 'list-last');
		await listStep('Alt+Down', last, 'list-end-page-noop');
		await listStep('Alt+Up', Math.max(0, last - page), 'list-page-up');
		await listStep('Alt+Shift+Up', 0, 'list-first');
		await listStep('Alt+Down', Math.min(last, page), 'list-page-down');
		const selectedBeforeHelp = selectedFixtureRow(current.grid, plan.rows);
		const openAck = await observeInput(expectedKey('F1', false), async () => {
			requireDelivered(await input.key('F1'));
		});
		await mark(openAck);
		const layout = helpLayout(current.grid, optionPickerHelpCensus);
		if (layout.kind === 'incomplete') throw new Error(layout.reason);
		observedHelpLayout = layout.kind;
		actions.push({ name: 'help-layout', kind: layout.kind });
		if (layout.kind === 'fit') {
			await (['Alt+Up', 'Alt+Down', 'Alt+Shift+Up', 'Alt+Shift+Down'] as const).reduce<
				Promise<void>
			>(async (previous, key) => {
				await previous;
				const before = await driver.waitForGrid('complete fitting Help before input', () => true);
				if (helpLayout(before, optionPickerHelpCensus).kind !== 'fit')
					throw new Error('Help no longer fits before no-op input');
				const ack = await observeInput(expectedKey(key, true), async () => {
					requireDelivered(await input.key(key));
				});
				await mark(ack);
				try {
					requireHelpNoop(before, current.grid);
				} catch (error) {
					throw new NavigationMismatch(String(error));
				}
				actions.push({ name: 'help-fit-noop', key });
				await capture('help-fit-' + key.replaceAll('+', '-'));
			}, Promise.resolve());
		} else {
			const initial = helpViewport(current.grid);
			if (!initial.rows.join(' ').includes('Context'))
				throw new Error('Help did not start at its first content');
			const full = [...initial.rows];
			const collectRows = async (remaining: number): Promise<void> => {
				const before = helpViewport(current.grid);
				if (before.atEnd) return;
				if (remaining === 0) throw new Error('bounded Help row qualification exhausted');
				const ack = await observeInput(expectedKey('Down', true), async () => {
					requireDelivered(await input.key('Down'));
				});
				await mark(ack);
				full.push(appendOneHelpRow(before.rows, helpViewport(current.grid).rows));
				await collectRows(remaining - 1);
			};
			await collectRows(2048);
			requireFullHelpCensus(full, optionPickerHelpCensus);
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
				const ack = await observeInput(expectedKey(key, true), async () => {
					requireDelivered(await input.key(key));
				});
				await mark(ack);
				try {
					requireHelpRows(helpViewport(current.grid).rows, expected);
				} catch (error) {
					throw new NavigationMismatch(`${name}: ${String(error)}`);
				}
				actions.push({ name, offset, viewportRows: initial.rows.length });
				await capture(name);
			};
			await helpStep('Alt+Shift+Up', 0, 'help-first');
			await helpStep('Alt+Shift+Up', 0, 'help-first-noop');
			await helpStep('Alt+Down', Math.min(maximum, initial.rows.length), 'help-page-down');
			await helpStep('Alt+Shift+Down', maximum, 'help-last');
			await helpStep('Alt+Down', maximum, 'help-end-page-noop');
			await helpStep('Alt+Up', Math.max(0, maximum - initial.rows.length), 'help-page-up');
		}
		const closeAck = await observeInput(expectedKey('Escape', true), async () => {
			requireDelivered(await input.key('Escape'));
		});
		await mark(closeAck);
		if (findDialog(current.grid, 'Help') !== null)
			throw new NavigationMismatch('Help remained open after Escape');
		if (selectedFixtureRow(current.grid, plan.rows) !== selectedBeforeHelp)
			throw new NavigationMismatch('Help changed the covered picker selection');
		await capture('help-return');
		await observeInput(expectedKey('Ctrl+C', false), async () => {
			requireDelivered(await input.key('Ctrl+C'));
		});
		// Loop-ending inputs may omit the frame marker. Settle on actual exit
		// and the fixture's returned outcome, never on a rendering guess.
		if ((await driver.drainUntilExit()) !== 0) throw new Error('picker did not exit cleanly');
		actions.push({
			name: 'quit-outcome',
			outcome: readSourceQualifiedOutcome(join(receiptsDirectory, 'picker-outcome.json'), {
				kind: 'cancelled',
			}),
		});
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
	if (observedHelpLayout === null) throw new Error('Help layout was not observed');
	return observedHelpLayout;
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
		helpLayout: 'fit' | 'overflow' | null;
	}[] = [];
	await plan.keys.reduce<Promise<void>>(async (previous, keys) => {
		await previous;
		await plan.grids.reduce<Promise<void>>(async (prior, grid) => {
			await prior;
			await checkOptionNavigationTarget(plan);
			let status: 'pass' | 'fail' | 'unobserved' = 'pass';
			let reason: string | null = null;
			let observedHelpLayout: 'fit' | 'overflow' | null = null;
			try {
				observedHelpLayout = await observeCase(plan, keys, grid);
			} catch (error) {
				if (error instanceof CleanupUnsettled) throw error;
				status =
					error instanceof NavigationMismatch ||
					/input acknowledgement .* mismatch/u.test(String(error))
						? 'fail'
						: 'unobserved';
				reason = String(error);
			}
			results.push({ keys, grid, status, reason, helpLayout: observedHelpLayout });
			writeFileSync(
				join(plan.output, 'results.json'),
				JSON.stringify({ source: plan.source.head, results }, null, 2),
			);
			process.stdout.write(`${keys} ${grid} ${status}${reason === null ? '' : ': ' + reason}\n`);
		}, Promise.resolve());
	}, Promise.resolve());
	await checkOptionNavigationTarget(plan);
	const helpCoverage = {
		overflowingCases: results.filter((row) => row.helpLayout === 'overflow').length,
		fittingCases: results.filter((row) => row.helpLayout === 'fit').length,
		unobservedCases: results.filter((row) => row.helpLayout === null).length,
	};
	writeFileSync(join(plan.output, 'help-coverage.json'), JSON.stringify(helpCoverage, null, 2));
	if (results.some((row) => row.status !== 'pass'))
		throw new Error('Option navigation includes contrary or unobserved cases');
	if (helpCoverage.overflowingCases === 0)
		throw new Error('No overflowing Help observed; paging coverage remains unproven');
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
