/** A launch frame must authorize typing; only the known update modal gets one Escape. */
import { homedir } from 'node:os';

import type { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import type { KeyProtocol } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import {
	CODEX_0_165_TEXT,
	UI_CHECKPOINT_MS,
	encodeCodexAction,
	gridShowsEmptyComposer,
} from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';
import { CODEX_PINNED_VERSION } from './codex-acp-target.ts';

function showsUpdateModal(lines: readonly string[]): boolean {
	// Only the update modal of the pinned CLI release is known; any other release is a stop.
	const title = `Update available · ${CODEX_PINNED_VERSION} → `;
	return (
		lines.some((line) => line.trim().startsWith(title)) &&
		['1. Update now', '2. Skip', '3. Skip until next version', 'enter continue', 'esc skip'].every(
			(text) => lines.some((line) => line.includes(text)),
		)
	);
}

/** Cursor focus and the final input pane distinguish the composer from a dialog backdrop. */
function composerReady(grid: TerminalGrid): boolean {
	const lines = gridLines(grid);
	const { cursor } = grid;
	const row = lines[cursor.row];
	if (!cursor.visible || cursor.col !== 2 || row === undefined || !gridShowsEmptyComposer([row]))
		return false;
	const below = lines
		.slice(cursor.row + 1)
		.map((line, index) => ({ line, offset: index + 1 }))
		.filter(({ line }) => line.trim() !== '');
	// The forced model status line may follow the active input; another input/view below it
	// means this composer is retained history rather than the bottom pane.
	return (
		below.length <= 1 && below.every(({ line, offset }) => offset <= 3 && /^ {2}\S/u.test(line))
	);
}

type StartupState = 'ready' | 'update' | 'resuming' | 'blocked';

/**
 * Real Codex 0.166.0's folder-trust prompt (D0 r1 attempt 4). The driver never answers it: the
 * owner adds trust entries himself (assignment Correction 27, review F1), so any trust prompt, for
 * any folder, stops the run with no key sent.
 */
function showsFolderTrustPrompt(lines: readonly string[]): boolean {
	return lines.some(
		(line) => line.trim() === 'Folder access' || line.includes('Trust this folder?'),
	);
}

/** How Codex prints the launch cwd in its startup header: the home directory as `~`. */
export function launchCwdLine(cwd: string, home: string = homedir()): string {
	return cwd === home || cwd.startsWith(`${home}/`) ? `~${cwd.slice(home.length)}` : cwd;
}

function startupState(grid: TerminalGrid, cwdLine: string): StartupState {
	const lines = gridLines(grid);
	if (showsUpdateModal(lines)) return 'update';
	if (showsFolderTrustPrompt(lines)) return 'blocked';
	if (
		[
			CODEX_0_165_TEXT.errorPrefix,
			CODEX_0_165_TEXT.working,
			CODEX_0_165_TEXT.modelPopupHeader,
			CODEX_0_165_TEXT.effortPopupTitle,
		].some((text) => lines.some((line) => line.includes(text)))
	)
		return 'blocked';
	if (lines.some((line) => line.includes(CODEX_0_165_TEXT.resumingSession))) {
		// Exactly the mcf50 startup view, with the declared launch cwd. Unknown added dialog/text
		// cannot gain a wait by retaining an old Resuming label behind it.
		const allowed = new Set([
			'',
			'>_ OpenAI Codex (v0.165.0)',
			'>_ OpenAI Codex (v0.166.0)',
			cwdLine,
			// Real Codex prints this placeholder in the header's directory line until the resumed thread
			// loads (D0 r1 frame, runs/r1-base-codex-reference/out-attempt2).
			'loading',
			CODEX_0_165_TEXT.resumingSession,
			CODEX_0_165_TEXT.composerPrompt + CODEX_0_165_TEXT.emptyComposerPlaceholder,
		]);
		return lines.every((line) => allowed.has(line.trim())) ? 'resuming' : 'blocked';
	}
	return composerReady(grid) ? 'ready' : 'blocked';
}

/** The default startup bound; a target may declare a longer one (real Codex resumes slower). */
export const STARTUP_COMPOSER_BOUND_MS = UI_CHECKPOINT_MS;

export async function requireLaunchComposer(inputs: {
	readonly driver: Pick<PtyGridDriver, 'frameGeneration' | 'send' | 'waitForGrid'>;
	readonly keys: KeyProtocol;
	readonly record: (step: CodexJourney['steps'][number], grid: TerminalGrid) => Promise<void>;
	/** The `--cd` directory Codex was launched in; its startup header shows it. */
	readonly launchCwd: string;
	/** The bound for the whole startup, modal dismissal included; defaults to the UI checkpoint bound. */
	readonly boundMs?: number;
	/**
	 * A readiness the launch must also reach before a composer frame counts (the real-Codex
	 * reference: its own thread/start reply on the wire, assignment Correction 30). Real Codex
	 * draws a transient composer, then its update modal, and starts its thread only after that, so
	 * a composer frame before it is not ready. `ready` may throw to refuse at once.
	 */
	readonly launchReadiness?: { readonly ready: () => boolean; readonly pending: () => string };
}): Promise<void> {
	const { driver, keys, record } = inputs;
	const cwdLine = launchCwdLine(inputs.launchCwd);
	const readiness = inputs.launchReadiness ?? null;
	// A launch frame is input-ready only when its composer is drawn and any declared readiness holds.
	const stateOf = (grid: TerminalGrid): StartupState => {
		const state = startupState(grid, cwdLine);
		return state === 'ready' && readiness !== null && !readiness.ready() ? 'resuming' : state;
	};
	const capture = async (scene: string, grid: TerminalGrid): Promise<void> => {
		await record(
			{
				actions: [],
				checkpoint: {
					scene,
					require: [
						scene === 'startup-update-modal'
							? 'Update available'
							: scene === 'startup-composer-ready'
								? CODEX_0_165_TEXT.emptyComposerPlaceholder
								: (gridLines(grid).find((line) => line.trim() !== '') ?? 'no startup frame'),
					],
					forbid: [],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			grid,
		);
	};
	const deadline = Date.now() + (inputs.boundMs ?? STARTUP_COMPOSER_BOUND_MS);
	const observation: { lastFrame: TerminalGrid | null } = { lastFrame: null };
	const remaining = (): number => {
		const milliseconds = deadline - Date.now();
		if (milliseconds <= 0) throw new Error('startup composer readiness bound reached');
		return milliseconds;
	};
	const waitForInput = async (
		dismissal: 'not-dismissed' | 'dismissed',
		after: number,
	): Promise<void> => {
		const grid = await driver.waitForGrid(
			'startup-composer-readiness',
			(candidate) => {
				observation.lastFrame = candidate;
				// Before the first completed draw the decoder has only its blank initial grid.
				if (
					driver.frameGeneration() === 0 &&
					gridLines(candidate).every((line) => line.trim() === '')
				)
					return false;
				return stateOf(candidate) !== 'resuming';
			},
			after,
			remaining(),
		);
		remaining();
		const state = stateOf(grid);
		if (state === 'update' && dismissal === 'not-dismissed') {
			await capture('startup-update-modal', grid);
			remaining();
			const generation = driver.frameGeneration();
			await driver.send(encodeCodexAction({ kind: 'key', key: 'Escape' }, keys));
			return waitForInput('dismissed', generation);
		}
		if (state !== 'ready')
			throw new Error(
				dismissal === 'dismissed'
					? 'composer is not ready after one Escape; no further keys'
					: 'composer is not ready; no keys sent',
			);
		await capture('startup-composer-ready', grid);
	};
	try {
		await waitForInput('not-dismissed', -1);
	} catch (error) {
		if (observation.lastFrame !== null)
			await capture('startup-composer-refused', observation.lastFrame);
		if (error instanceof Error && error.message.includes('composer is not ready')) throw error;
		if (error instanceof Error && error.message.startsWith('the real-Codex reference')) throw error;
		if (readiness !== null && !readiness.ready())
			throw new Error(
				`the launch did not reach its declared readiness within its startup bound (${readiness.pending()}); no input sent`,
				{ cause: error },
			);
		throw new Error('startup composer readiness did not reach a ready frame within its bound', {
			cause: error,
		});
	}
}
