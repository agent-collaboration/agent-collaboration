/** A launch frame must authorize typing; only the known update modal gets one Escape. */
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

function showsUpdateModal(lines: readonly string[]): boolean {
	return (
		lines.some((line) => /Update available\s*·\s*0\.165\.0\s*→\s*\S+/u.test(line)) &&
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

function startupState(grid: TerminalGrid): StartupState {
	const lines = gridLines(grid);
	if (showsUpdateModal(lines)) return 'update';
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
		// Exactly the mcf50 startup view, including its known cwd. Unknown added dialog/text
		// cannot gain a wait by retaining an old Resuming label behind it.
		const allowed = new Set([
			'',
			'>_ OpenAI Codex (v0.165.0)',
			'~/code/projects/codex-router',
			CODEX_0_165_TEXT.resumingSession,
			CODEX_0_165_TEXT.composerPrompt + CODEX_0_165_TEXT.emptyComposerPlaceholder,
		]);
		return lines.every((line) => allowed.has(line.trim())) ? 'resuming' : 'blocked';
	}
	return composerReady(grid) ? 'ready' : 'blocked';
}

const STARTUP_COMPOSER_BOUND_MS = UI_CHECKPOINT_MS;

export async function requireLaunchComposer(inputs: {
	readonly driver: Pick<PtyGridDriver, 'frameGeneration' | 'send' | 'waitForGrid'>;
	readonly keys: KeyProtocol;
	readonly record: (step: CodexJourney['steps'][number], grid: TerminalGrid) => Promise<void>;
}): Promise<void> {
	const { driver, keys, record } = inputs;
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
	const deadline = Date.now() + STARTUP_COMPOSER_BOUND_MS;
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
				return startupState(candidate) !== 'resuming';
			},
			after,
			remaining(),
		);
		remaining();
		const state = startupState(grid);
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
		throw new Error('startup composer readiness did not reach a ready frame within its bound', {
			cause: error,
		});
	}
}
