/** One retained Rust-owned PTY, the existing wrapper, and exact causal receipts. */
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { ProcessIdentity } from '../evidence-schema.ts';
import { GhosttyJourneyInput, requireDelivered } from '../ghostty-journey-input.ts';
import {
	InputConsumptionReadback,
	parseInputConsumptionReadback,
} from '../input-consumption-readback.ts';
import type {
	InputConsumptionExpectation,
	InputEventReadback,
} from '../input-consumption-schema.ts';
import { lookupProcessIdentity, sameIdentity } from '../process-identity.ts';
import { PtyGridDriver } from '../pty-grid-driver.ts';
import type { KeyProtocol } from '../pty-grid-driver.ts';
import { canonicalJson, fileDigest, sourceDigest } from '../source-binding.ts';
import { gridLines } from '../terminal-grid.ts';
import type { TerminalGrid } from '../terminal-grid.ts';
import { captureShowcase } from './capture-showcase.ts';
import { PrerequisiteGap, ScenarioMismatch, requireObservation } from './contracts.ts';
import type { ShowcasePlan, ShowcaseState } from './contracts.ts';
import { pointForTarget, pointForScroll, pointOutsideSurface } from './hit-map-points.ts';
import { expectedInput, expectedKey, expectedClick } from './input-expectations.ts';
import type { ExpectedRoute } from './input-expectations.ts';
import { mainMeasurementFollowUp, needsMeasurementDraw, viewScope } from './measurement-binding.ts';
import { ReceiptNotifications } from './receipt-notifications.ts';
import { actionPresentation, currentRegistry, visibleTarget } from './scenario-helpers.ts';
import { observeState, ShowcaseJournal, serializedState } from './state-reader.ts';
import { serializedFrameCount } from './synchronized-frames.ts';

export class ShowcaseRig {
	public readonly driver: PtyGridDriver;
	public readonly input: GhosttyJourneyInput;
	public readonly directory: string;
	public readonly keys: KeyProtocol;
	private readonly plan: ShowcasePlan;
	private readonly reader: InputConsumptionReadback;
	private readonly notifications: ReceiptNotifications;
	private readonly journal: ShowcaseJournal;
	private readonly childIdentity: ProcessIdentity;
	private highestOverlay = 0n;
	private completed = false;
	private visibleState: ShowcaseState | null = null;
	private readonly observations: unknown[] = [];
	private constructor(options: {
		readonly driver: PtyGridDriver;
		readonly plan: ShowcasePlan;
		readonly directory: string;
		readonly keys: KeyProtocol;
		readonly reader: InputConsumptionReadback;
		readonly child: ProcessIdentity;
		readonly notifications: ReceiptNotifications;
	}) {
		this.driver = options.driver;
		this.plan = options.plan;
		this.directory = options.directory;
		this.keys = options.keys;
		this.reader = options.reader;
		this.notifications = options.notifications;
		this.childIdentity = options.child;
		this.journal = new ShowcaseJournal(join(options.directory, 'receipts', 'showcase-state.jsonl'));
		this.input = new GhosttyJourneyInput(options.driver, options.plan.inputProfile);
	}
	public static async start(options: {
		readonly plan: ShowcasePlan;
		readonly directory: string;
		readonly keys: KeyProtocol;
		readonly columns: number;
		readonly rows: number;
	}): Promise<ShowcaseRig> {
		mkdirSync(options.directory);
		const receipts = join(options.directory, 'receipts');
		mkdirSync(receipts);
		const configuration = join(options.directory, 'fixture-command.json');
		writeFileSync(
			configuration,
			JSON.stringify({
				surface: 'showcase',
				binary: options.plan.example.path,
				binary_sha256: options.plan.example.sha256,
				approved_cwd: options.plan.repository,
				output: options.directory,
				temporary_directory: options.directory,
				receipt_directory: receipts,
			}),
		);
		const notifications = new ReceiptNotifications(receipts);
		let driver: PtyGridDriver | null = null;
		try {
			driver = await PtyGridDriver.start({
				repository: options.plan.repository,
				driver: options.plan.driver.path,
				surface: 'showcase',
				keys: options.keys,
				evidence: options.directory,
				showcaseEnvironment: {
					QUOTA_STORYBOARD_NODE: options.plan.node.path,
					QUOTA_STORYBOARD_NODE_SHA256: options.plan.node.sha256,
					TUI_SHOWCASE_OUTPUT: options.directory,
					TUI_SHOWCASE_CONFIGURATION: configuration,
					TUI_SHOWCASE_CONFIGURATION_SHA256: fileDigest(configuration),
				},
			});
			await driver.waitForGrid('showcase entry', (grid) =>
				gridLines(grid).some((line) => line.includes('Component showcase')),
			);
			const child = lookupProcessIdentity(
				Number(readFileSync(join(options.directory, 'quota-child.pid'), 'utf8').trim()),
			);
			if (
				child === null ||
				child.realUid !== process.getuid?.() ||
				realpathSync(child.executable) !== options.plan.example.path
			)
				throw new PrerequisiteGap('sample child identity does not match its declaration');
			const reader = InputConsumptionReadback.open(join(receipts, 'input-dispatch.jsonl'));
			await notifications.ready();
			const rig = new ShowcaseRig({
				driver,
				plan: options.plan,
				directory: options.directory,
				keys: options.keys,
				reader,
				notifications,
				child,
			});
			rig.state();
			await rig.observeDraw({ minimumFrame: 2, inputOrdinal: 0 });
			writeFileSync(
				join(options.directory, 'child-binding.json'),
				JSON.stringify(
					{
						kind: 'local-component-showcase',
						child,
						node: options.plan.node,
						example: options.plan.example,
						driver: options.plan.driver,
						sourceDigest: sourceDigest(options.plan.source),
					},
					null,
					2,
				),
			);
			if (options.columns !== 120 || options.rows !== 36)
				await rig.resize(options.columns, options.rows);
			return rig;
		} catch (error) {
			await driver?.stop();
			notifications.close();
			throw error;
		}
	}
	public state(): ShowcaseState {
		const state = this.journal.read().states.at(-1);
		if (state === undefined)
			throw new PrerequisiteGap('sample state file has no complete observation');
		for (const overlay of state.overlays)
			this.highestOverlay =
				BigInt(overlay.overlayId) > this.highestOverlay
					? BigInt(overlay.overlayId)
					: this.highestOverlay;
		return state;
	}
	public nextOverlaySurface(): string {
		return 'overlay:' + String(this.highestOverlay + 1n);
	}
	public route(action: string | null = null, target: string | null = null): ExpectedRoute {
		return { surface: this.state().surface, action, target };
	}
	public async settleFrame(
		minimumObservation = 0,
		inputOrdinal = this.reader.sequence,
	): Promise<ShowcaseState> {
		return this.observeDraw({ minimumObservation, inputOrdinal });
	}
	public drawState(): ShowcaseState {
		if (this.visibleState === null) throw new PrerequisiteGap('no serialized received draw bound');
		return this.visibleState;
	}
	public async awaitVisibleTarget(target: string): Promise<void> {
		await this.observeDraw({
			inputOrdinal: this.reader.sequence,
			matches: (state) =>
				state.hitMap?.targets.some(
					(region) =>
						region.surface === state.surface && region.target === target && region.cells.length > 0,
				) === true,
		});
	}
	private async observeDraw(options: {
		readonly minimumObservation?: number;
		readonly inputOrdinal?: number;
		readonly minimumFrame?: number;
		readonly matches?: (state: ShowcaseState) => boolean;
	}): Promise<ShowcaseState> {
		let observed: ShowcaseState | null = null;
		await this.driver.waitForGrid('exact serialized example frame', () => {
			const journal = this.journal.read();
			const physical = serializedFrameCount(this.driver.bytes());
			observed = serializedState({
				journal,
				receivedFrames: physical,
				inputOrdinal: options.inputOrdinal ?? this.reader.sequence,
				minimumObservation: options.minimumObservation ?? 0,
			});
			return (
				observed !== null &&
				physical >= (options.minimumFrame ?? 0) &&
				observed.hitMap !== null &&
				observed.hitMap.surfaces.some((surface) => surface.surface === observed?.surface) &&
				(options.matches?.(observed) ?? true)
			);
		});
		if (observed === null) throw new PrerequisiteGap('serialized frame has no matching state');
		this.state();
		this.visibleState = observed;
		return observed;
	}
	private async inject(
		expected: readonly InputConsumptionExpectation[],
		inject: () => Promise<void>,
		redraw: boolean | { readonly throughInput: number },
	): Promise<ShowcaseState> {
		const before = this.reader.sequence;
		const previousState = this.state();
		const previousReceivedFrame = serializedFrameCount(this.driver.bytes());
		const previousFrame = this.journal.read().serializations.at(-1)?.sequence ?? 0;
		try {
			await this.reader.observeInput({
				expected,
				timeoutMs: 8000,
				inject: () =>
					this.notifications
						.wait({
							description: 'complete producer input records',
							inject,
							matches: () => {
								const bytes = readFileSync(
									join(this.directory, 'receipts', 'input-dispatch.jsonl'),
								);
								if (bytes.length > 0 && bytes.at(-1) !== 10) return null;
								return parseInputConsumptionReadback(bytes).length >= before + expected.length
									? true
									: null;
							},
						})
						.then(() => {}),
			});
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			const actual = parseInputConsumptionReadback(
				readFileSync(join(this.directory, 'receipts', 'input-dispatch.jsonl')),
			).slice(before);
			if (/input acknowledgement (?:surface|target|action) mismatch/u.test(message))
				throw new ScenarioMismatch(message, { expected, actual });
			throw new PrerequisiteGap(message);
		}
		const state = this.state();
		requireObservation(
			state.inputOrdinal === this.reader.sequence,
			'callback state must precede its matching dispatch ACK',
			{ stateOrdinal: state.inputOrdinal, inputSequence: this.reader.sequence },
		);
		this.observations.push({
			sequence: this.reader.sequence,
			expected,
			stateObservation: state.observation,
		});
		if (!redraw) return state;
		const inputOrdinal =
			before + (typeof redraw === 'object' ? redraw.throughInput : expected.length);
		let painted = await this.observeDraw({ inputOrdinal });
		if (needsMeasurementDraw(previousState, state)) {
			const journal = this.journal.read();
			const first = journal.serializations.find((record) => {
				const draw = journal.states[record.drawObservation - 1];
				return (
					record.sequence > previousFrame &&
					record.inputOrdinal >= inputOrdinal &&
					draw?.surface === state.surface &&
					viewScope(draw) === viewScope(state)
				);
			});
			if (first === undefined)
				throw new PrerequisiteGap('new measured view has no corresponding first draw');
			this.observations.push({
				kind: 'measurement-follow-up',
				firstFrame: first.sequence,
				minimumFrame: first.sequence + 1,
				surface: state.surface,
				scope: viewScope(state),
			});
			painted = await this.observeDraw({ inputOrdinal, minimumFrame: first.sequence + 1 });
		}
		const journal = this.journal.read();
		const minimumMeasuredFrame = mainMeasurementFollowUp(
			journal.serializations.map((frame) => ({
				sequence: frame.sequence,
				layout: journal.states[frame.drawObservation - 1]?.mainScrollLayout ?? null,
			})),
			previousReceivedFrame,
		);
		if (minimumMeasuredFrame > 0) {
			this.observations.push({
				kind: 'main-measurement-follow-up',
				minimumFrame: minimumMeasuredFrame,
			});
			painted = await this.observeDraw({ inputOrdinal, minimumFrame: minimumMeasuredFrame });
		}
		return painted;
	}
	public async key(
		key: string,
		route: ExpectedRoute = this.route(),
		redraw = true,
	): Promise<ShowcaseState> {
		return this.inject(
			[expectedKey(key, route)],
			async () => {
				requireDelivered(await this.input.key(key));
			},
			redraw,
		);
	}
	public async rawKey(options: {
		readonly bytes: Buffer;
		readonly event: InputEventReadback;
		readonly route: ExpectedRoute;
		readonly redraw?: boolean;
		readonly label: string;
	}): Promise<ShowcaseState> {
		this.observations.push({
			kind: 'declared-vt-supplement',
			label: options.label,
			hex: options.bytes.toString('hex'),
			modes: this.driver.inputModes(),
		});
		return this.inject(
			[expectedInput(options.event, options.route)],
			() => this.driver.send(options.bytes),
			options.redraw ?? true,
		);
	}
	public async burst(options: {
		readonly bytes: Buffer;
		readonly expected: readonly InputConsumptionExpectation[];
		readonly label: string;
		readonly redraw?: boolean;
		readonly frameThroughInput?: number;
	}): Promise<ShowcaseState> {
		this.observations.push({
			kind: 'declared-vt-burst',
			label: options.label,
			hex: options.bytes.toString('hex'),
			modes: this.driver.inputModes(),
		});
		return this.inject(
			options.expected,
			() => this.driver.send(options.bytes),
			options.frameThroughInput === undefined
				? (options.redraw ?? true)
				: { throughInput: options.frameThroughInput },
		);
	}
	public async wheel(
		target: string,
		direction: 'up' | 'down',
		redraw = true,
	): Promise<ShowcaseState> {
		await this.observeDraw({ inputOrdinal: 0 });
		const { column, row } = pointForScroll(this.drawState(), target);
		return this.inject(
			[
				expectedInput(
					{
						kind: 'mouse',
						mouse: { kind: direction === 'up' ? 'scrollUp' : 'scrollDown' },
						column,
						row,
						modifiers: [],
					},
					this.route(null, target),
				),
			],
			async () => {
				requireDelivered(await this.input.mouse({ kind: 'wheel', direction, column, row }));
			},
			redraw,
		);
	}
	public async text(text: string, target: string, redraw = true): Promise<ShowcaseState> {
		const route = this.route(null, target);
		return this.inject(
			Array.from(text, (character) =>
				expectedInput(
					{
						kind: 'key',
						code: { kind: 'text' },
						modifiers: /^[A-Z]$/u.test(character) ? ['shift'] : [],
						phase: 'press',
					},
					route,
				),
			),
			async () => {
				requireDelivered(await this.input.text(text));
			},
			redraw,
		);
	}
	public async paste(text: string, target: string): Promise<ShowcaseState> {
		return this.inject(
			[expectedInput({ kind: 'other', event: 'bracketedPaste' }, this.route(null, target))],
			() => this.driver.send(Buffer.from('\x1b[200~' + text + '\x1b[201~')),
			true,
		);
	}
	public async resize(columns: number, rows: number): Promise<ShowcaseState> {
		this.sameChild();
		await this.observeDraw({ inputOrdinal: 0 });
		if (this.state().viewport[0] === columns && this.state().viewport[1] === rows)
			return this.state();
		const state = await this.inject(
			[expectedInput({ kind: 'resize', columns, rows }, this.route())],
			() => this.driver.resize(columns, rows),
			true,
		);
		this.sameChild();
		await this.observeDraw({ inputOrdinal: 0 });
		requireObservation(
			state.viewport[0] === columns && state.viewport[1] === rows,
			'same-child viewport must match kernel resize',
			state.viewport,
		);
		return state;
	}
	public sameChild(): void {
		const current = lookupProcessIdentity(this.childIdentity.pid);
		if (current === null || !sameIdentity(current, this.childIdentity))
			throw new PrerequisiteGap('retained example child identity changed');
	}
	public async completeFixture(): Promise<ShowcaseState> {
		const before = this.state();
		if (!before.completionReady)
			throw new PrerequisiteGap('owned sample completion listener is not ready');
		const state = await observeState({
			notifications: this.notifications,
			journal: this.journal,
			description: 'controlled fixture completion',
			matches: (candidate) =>
				candidate.cause === 'fixture' && candidate.completionCount === before.completionCount + 1,
			inject: async () => {
				this.sameChild();
				process.kill(this.childIdentity.pid, 'SIGUSR1');
			},
		});
		return this.settleFrame(state.observation + 1);
	}
	public async focus(target: string): Promise<void> {
		const move = async (remaining: number, seen: readonly (string | null)[]): Promise<void> => {
			const state = this.state();
			if (state.focus === target) return;
			if (remaining === 0 || seen.includes(state.focus))
				throw new PrerequisiteGap('Tab cannot reach declared target ' + target);
			await this.key('Tab');
			await move(remaining - 1, [...seen, state.focus]);
		};
		await move(64, []);
	}
	public async menu(): Promise<void> {
		await this.key('F10', this.route('menu'));
		requireObservation(
			this.state().overlays.at(-1)?.kind === 'menu',
			'F10 opens a menu surface',
			this.state().overlays,
		);
	}
	public async menuAction(action: string, via: 'keyboard' | 'mouse' = 'keyboard'): Promise<void> {
		if (this.state().overlays.at(-1)?.kind !== 'menu') {
			if (via === 'mouse') await this.click('action:menu', 'menu', { opens: true });
			else await this.menu();
		}
		const target = 'action:' + action;
		if (via === 'mouse') {
			const reveal = async (remaining: number): Promise<void> => {
				await this.observeDraw({ inputOrdinal: 0 });
				if (visibleTarget(this.drawState(), target)) return;
				if (remaining === 0) throw new PrerequisiteGap('Menu wheel cannot reveal ' + action);
				const after = await this.wheel('menu-scroll', 'down', false);
				if (after.hitMap !== null)
					throw new PrerequisiteGap('Menu wheel reached its end before ' + action);
				await this.settleFrame();
				await reveal(remaining - 1);
			};
			await reveal(currentRegistry(this.state()).actions.length);
			const ignored = actionPresentation(this.state(), action).availability.kind === 'unavailable';
			const opens = [
				'form',
				'details',
				'percentage',
				'fill',
				'fork',
				'help',
				'screen-choice-open',
			].includes(action);
			await this.click(target, action, {
				opens: opens && !ignored,
				closes: !opens && !ignored,
				ignored,
			});
			return;
		}
		const move = async (remaining: number): Promise<void> => {
			if (this.state().focus === target) return;
			if (remaining === 0) throw new PrerequisiteGap('Menu cannot reach action ' + action);
			await this.key('Down');
			await move(remaining - 1);
		};
		await move(64);
		await this.key('Enter', this.route(action, target));
	}
	public point(target: string): { readonly column: number; readonly row: number } {
		return pointForTarget(this.drawState(), target);
	}
	public completedDrawBetweenInputs(first: number, second: number): boolean {
		const states = this.journal.read().states;
		const nextInput = states.find(
			(state) => state.cause === 'input' && state.inputOrdinal === second,
		);
		if (nextInput === undefined)
			throw new PrerequisiteGap('second input callback was not retained');
		const draws = states.filter(
			(state) =>
				state.cause === 'frame' &&
				state.inputOrdinal === first &&
				state.observation < nextInput.observation,
		);
		this.observations.push({
			kind: 'draws-between-inputs',
			first,
			second,
			draws: draws.map((state) => state.observation),
		});
		return draws.length > 0;
	}
	public async click(
		target: string,
		action: string | null,
		options: {
			readonly closes?: boolean;
			readonly opens?: boolean;
			readonly ignored?: boolean;
		} = {},
	): Promise<ShowcaseState> {
		await this.observeDraw({ inputOrdinal: 0 });
		const before = this.state();
		const point = this.point(target);
		const parent =
			before.overlays.length > 1 ? 'overlay:' + before.overlays.at(-2)?.overlayId : 'screen';
		const releaseSurface = options.closes
			? parent
			: options.opens
				? this.nextOverlaySurface()
				: before.surface;
		const pressRoute = {
			surface: before.surface,
			target: options.ignored ? null : target,
			action: options.ignored ? null : action,
		};
		const expected = expectedClick({ point, pressRoute, releaseSurface });
		return this.inject(
			expected,
			async () => {
				requireDelivered(await this.input.mouse({ kind: 'press', button: 'left', ...point }));
				requireDelivered(await this.input.mouse({ kind: 'release', button: 'left', ...point }));
			},
			options.ignored ? false : { throughInput: 1 },
		);
	}
	public async clickOutside(): Promise<ShowcaseState> {
		await this.observeDraw({ inputOrdinal: 0 });
		const before = this.state();
		const point = pointOutsideSurface(this.drawState(), this.grid());
		const closes = before.overlays.at(-1)?.kind !== 'dialog';
		const parent =
			before.overlays.length > 1 ? 'overlay:' + before.overlays.at(-2)?.overlayId : 'screen';
		return this.inject(
			expectedClick({
				point,
				pressRoute: this.route(),
				releaseSurface: closes ? parent : before.surface,
			}),
			async () => {
				requireDelivered(await this.input.mouse({ kind: 'press', button: 'left', ...point }));
				requireDelivered(await this.input.mouse({ kind: 'release', button: 'left', ...point }));
			},
			closes ? { throughInput: 1 } : false,
		);
	}
	public grid(): TerminalGrid {
		return this.driver.grid();
	}
	public lines(): readonly string[] {
		return gridLines(this.grid());
	}
	public async capture(name: string): Promise<void> {
		await this.observeDraw({ inputOrdinal: 0 });
		await captureShowcase({
			directory: this.directory,
			driver: this.driver,
			plan: this.plan,
			state: this.state(),
			drawState: this.drawState(),
			name,
		});
	}
	public requireText(text: string): void {
		if (!this.lines().some((line) => line.includes(text)))
			throw new PrerequisiteGap('required visible sample feature is absent: ' + text);
	}
	public async quit(): Promise<void> {
		await this.key('Ctrl+C', { surface: 'screen', target: null, action: 'quit' }, false);
		const drain = async (): Promise<void> => {
			if ((await this.driver.receive()) !== 'eof') await drain();
		};
		await drain();
		requireObservation((await this.driver.finish()) === 0, 'sample Quit exits successfully', null);
		this.reader.assertComplete();
		const finalJournal = this.journal.read(true);
		requireObservation(
			finalJournal.serializations.length === serializedFrameCount(this.driver.bytes()),
			'final serialized-frame count equals actual paired PTY frames',
			{
				receipts: finalJournal.serializations.length,
				received: serializedFrameCount(this.driver.bytes()),
			},
		);
		this.completed = true;
	}
	public async stop(): Promise<void> {
		await this.driver.stop();
		this.notifications.close();
		writeFileSync(
			join(this.directory, 'observer-receipt.json'),
			JSON.stringify(
				{
					kind: 'showcase-observer',
					completed: this.completed,
					inputProfile: this.plan.inputProfile,
					delivery: this.input.history(),
					observations: this.observations,
					notifications: this.notifications.history(),
					child: this.childIdentity,
				},
				null,
				2,
			),
		);
	}
	public unchanged(
		before: ShowcaseState,
		fields: readonly (keyof ShowcaseState)[],
		message: string,
	): void {
		const after = this.state();
		for (const field of fields)
			requireObservation(
				canonicalJson(after[field]) === canonicalJson(before[field]),
				message + ' (' + field + ')',
				{ before: before[field], after: after[field] },
			);
	}
}
