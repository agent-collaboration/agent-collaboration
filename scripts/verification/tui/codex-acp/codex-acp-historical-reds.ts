/**
 * Baseline-only continuation after a named historical red (assignment Correction 21). A baseline
 * run may name the steps the catalog expects to fail at that build, each with its reason and the
 * error cell that shows it. Each such step is still driven exactly as declared; when it fails and
 * its named error cell is on screen, the red is recorded as a failure and the remaining steps run
 * so their results (status line, provider usage) are decided in the same run. Those later steps
 * are recorded with what their final frame showed instead of stopping the run. A named step that
 * passes, or fails without its error cell, is the plan's stop condition: the baseline differs from
 * the catalog. A head run names no reds and stays strict.
 */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import type { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { UI_CHECKPOINT_MS, gridShowsCheckpoint } from './codex-acp-scenes.ts';
import type { CodexCheckpoint, CodexJourney } from './codex-acp-scenes.ts';

export const HISTORICAL_REDS_FILE = 'historical-reds.json';

/** Which build a run drives: only a baseline run may name historical reds. */
export const runBuildSchema = z.enum(['baseline', 'head']);

export const historicalRedSchema = z.strictObject({
	// The declared step the catalog expects to fail at this build.
	scene: z.string().regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/u),
	// The named reason, as the Lead recorded it.
	reason: z.string().min(1),
	// The whole error history cell that shows this red (its text after `■ `).
	error_line: z.string().min(1),
});
export type HistoricalRed = z.infer<typeof historicalRedSchema>;

type Step = CodexJourney['steps'][number];
type StepDriver = Pick<PtyGridDriver, 'frameGeneration' | 'send' | 'waitForGrid'>;

/** What a step after a red showed on its final frame. */
export interface ContinuedStepOutcome {
	readonly scene: string;
	readonly passed: boolean;
	readonly present_require: readonly string[];
	readonly missing_require: readonly string[];
	readonly present_forbid: readonly string[];
}

export interface ReproducedRed {
	readonly scene: string;
	readonly reason: string;
	readonly error_line: string;
	// The step's own checkpoint failure, kept verbatim.
	readonly step_failure: string;
}

/** Each red's scene must be a declared step, named once. */
export function requireRedsInJourney(reds: readonly HistoricalRed[], journey: CodexJourney): void {
	const scenes = new Set(journey.steps.map((step) => step.checkpoint.scene));
	const named = new Set<string>();
	for (const red of reds) {
		if (!scenes.has(red.scene))
			throw new Error(`historical red names no declared step: ${red.scene}`);
		if (named.has(red.scene)) throw new Error(`historical red named twice: ${red.scene}`);
		named.add(red.scene);
	}
}

/** The frame that shows a red: its whole error cell. */
export function redCheckpoint(red: HistoricalRed): CodexCheckpoint {
	return {
		scene: `${red.scene}-historical-red`,
		require: [red.error_line],
		forbid: [],
		deadline_ms: UI_CHECKPOINT_MS,
		error_line: red.error_line,
	};
}

export function continuedStepOutcome(step: Step, grid: TerminalGrid): ContinuedStepOutcome {
	const lines = gridLines(grid);
	const shows = (text: string): boolean => lines.some((line) => line.includes(text));
	const { checkpoint } = step;
	return {
		scene: checkpoint.scene,
		passed: gridShowsCheckpoint(lines, checkpoint),
		present_require: checkpoint.require.filter(shows),
		missing_require: checkpoint.require.filter((text) => !shows(text)),
		present_forbid: checkpoint.forbid.filter(shows),
	};
}

/**
 * Drives one baseline run's steps against its named reds. `driveStep` is the strict step driver;
 * `record` keeps each frame. Returns the scenes recorded, in order.
 */
export class HistoricalRedContinuation {
	readonly #reds: ReadonlyMap<string, HistoricalRed>;
	readonly #reproduced: ReproducedRed[] = [];
	readonly #continued: ContinuedStepOutcome[] = [];

	constructor(reds: readonly HistoricalRed[]) {
		this.#reds = new Map(reds.map((red) => [red.scene, red]));
	}

	get active(): boolean {
		return this.#reds.size > 0;
	}

	/** Whether a red has been recorded, so later steps no longer stop the run. */
	get continuing(): boolean {
		return this.#reproduced.length > 0;
	}

	async driveStep(inputs: {
		readonly driver: StepDriver;
		readonly step: Step;
		readonly driveStep: (driver: StepDriver, step: Step) => Promise<TerminalGrid>;
		readonly record: (step: Step, grid: TerminalGrid) => Promise<void>;
	}): Promise<string> {
		const { driver, step } = inputs;
		const red = this.#reds.get(step.checkpoint.scene);
		if (red !== undefined) return this.#driveRed(inputs, red);
		if (!this.continuing) {
			await inputs.record(step, await inputs.driveStep(driver, step));
			return step.checkpoint.scene;
		}
		// After a red: the step runs as declared; its final frame is kept and judged, not gated.
		const holder: { last: TerminalGrid | null } = { last: null };
		const observing = {
			...driver,
			waitForGrid: (
				name: string,
				matches: (grid: TerminalGrid) => boolean,
				after?: number,
				bound?: number,
			): Promise<TerminalGrid> =>
				driver.waitForGrid(
					name,
					(grid) => {
						holder.last = grid;
						return matches(grid);
					},
					after,
					bound,
				),
		};
		const grid = await inputs
			.driveStep(observing, step)
			.catch(
				async (): Promise<TerminalGrid> =>
					holder.last ?? (await driver.waitForGrid(step.checkpoint.scene, () => true, -1, 1)),
			);
		await inputs.record(step, grid);
		this.#continued.push(continuedStepOutcome(step, grid));
		return step.checkpoint.scene;
	}

	async #driveRed(
		inputs: {
			readonly driver: StepDriver;
			readonly step: Step;
			readonly driveStep: (driver: StepDriver, step: Step) => Promise<TerminalGrid>;
			readonly record: (step: Step, grid: TerminalGrid) => Promise<void>;
		},
		red: HistoricalRed,
	): Promise<string> {
		const { driver, step } = inputs;
		let stepFailure: string;
		try {
			await inputs.driveStep(driver, step);
			stepFailure = '';
		} catch (error) {
			stepFailure = error instanceof Error ? error.message : String(error);
		}
		if (stepFailure === '')
			throw new Error(
				`historical red did not reproduce: ${red.scene} passed at this build (${red.reason})`,
			);
		const checkpoint = redCheckpoint(red);
		const grid = await driver
			.waitForGrid(
				checkpoint.scene,
				(candidate) => gridShowsCheckpoint(gridLines(candidate), checkpoint),
				-1,
				checkpoint.deadline_ms,
			)
			.catch(() => {
				throw new Error(
					`${red.scene} failed without its named error cell "${red.error_line}": ${stepFailure}`,
				);
			});
		await inputs.record({ actions: [], checkpoint }, grid);
		this.#reproduced.push({
			scene: red.scene,
			reason: red.reason,
			error_line: red.error_line,
			step_failure: stepFailure,
		});
		return checkpoint.scene;
	}

	/** Every named red must have reproduced; the record is written either way. */
	settle(output: string): void {
		writeFileSync(
			join(output, HISTORICAL_REDS_FILE),
			JSON.stringify(
				{
					build: 'baseline',
					reds: this.#reproduced,
					continued_steps: this.#continued,
				},
				null,
				2,
			),
			{ mode: 0o600, flag: 'wx' },
		);
		const missing = [...this.#reds.keys()].filter(
			(scene) => !this.#reproduced.some((red) => red.scene === scene),
		);
		if (missing.length > 0)
			throw new Error(`named historical reds never reached: ${missing.join(',')}`);
	}

	/** The scenes a run with reds records: a red step's scene becomes its `-historical-red` frame. */
	expectedScenes(journey: CodexJourney): string[] {
		return journey.steps.map((step) =>
			this.#reds.has(step.checkpoint.scene)
				? `${step.checkpoint.scene}-historical-red`
				: step.checkpoint.scene,
		);
	}

	/** Each red's error cell is expected in the whole run, in addition to the journey's own. */
	expectedErrorCells(): string[] {
		return [...this.#reds.values()].map((red) => red.error_line);
	}
}
