/** Real streaming frame/driver boundary; the child is a controlled terminal, never Codex. */
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';
import { z } from 'zod';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { promptJourney } from './codex-acp-prompt-scenes.ts';
import { driveCodexJourney } from './codex-acp-run.ts';
import { skillJourney } from './codex-acp-scenes.ts';

afterEach(() => vi.restoreAllMocks());

const RESUMING = [
	'',
	'  >_ OpenAI Codex (v0.165.0)',
	'     ~/code/projects/codex-router',
	'  Resuming session…',
	'',
	'› Ask Codex to do anything',
];
const MEMORY_DIALOG = [
	'                 Enable memories?',
	'                 Yes, enable',
	'                 Not now',
	'',
	'› Ask Codex to do anything',
	'  haiku low',
];

const COMPOSER = ['› Ask Codex to do anything', '  haiku low'];
// Literal mcf49 launch modal, as reported by the Lead. Digits/Enter can invoke an installer.
const UPDATE = [
	'Update available · 0.165.0 → 0.167.0',
	'› 1. Update now (runs the installer)',
	'  2. Skip',
	'  3. Skip until next version',
	'enter continue · esc skip',
];

function frame(rows: readonly string[]): string {
	const composerRow = rows.findIndex((line) => line.startsWith('› Ask Codex'));
	const focus = rows.some((line) => line.includes('Enable memories?'))
		? '\u001b[2;18H\u001b[?25h'
		: composerRow >= 0
			? `\u001b[${composerRow + 1};3H\u001b[?25h`
			: '\u001b[?25l';
	return Buffer.from(
		`\u001b[?2026h\u001b[2J\u001b[H${rows.join('\r\n')}${focus}\u001b[?2026l`,
	).toString('hex');
}

async function launchFrameCase(inputs: {
	readonly kind: 'prompt' | 'skill';
	readonly initial: readonly string[];
	readonly afterEscape: readonly string[];
	readonly transitions?: readonly (readonly string[])[];
	readonly expireOnResume?: boolean;
	readonly expireOnReady?: boolean;
}): Promise<{
	readonly drive: () => Promise<string[]>;
	readonly sent: () => string[];
	readonly captured: { scene: string; lines: readonly string[] }[];
	readonly close: () => Promise<void>;
}> {
	const root = temporaryRoot();
	const prompt = 'Reply with only the product of 4321 and 5678, digits only';
	const skill = 'bridge_probe_claude_01a18013';
	const journey =
		inputs.kind === 'prompt'
			? promptJourney({
					provider: 'claude',
					prompt,
					completion: '24534638',
					replyDeadlineMs: 8_000,
				})
			: skillJourney({
					provider: 'claude',
					skills: [skill],
					prefix: '',
					args: 'ALPHA',
					expectedEffects: ['PROVIDER_REPLY_8K2J'],
					imagePath: null,
					effectDeadlineMs: 8_000,
				});
	const first = journey.steps[0];
	if (first === undefined) throw new Error('missing first input step');
	const typed = inputs.kind === 'prompt' ? prompt : `$${skill}`;
	const composed =
		inputs.kind === 'prompt'
			? [`› ${prompt}`]
			: [`› $${skill}`, `› ${skill}    [Skill]`, 'enter insert · esc close'];
	const log = join(root, 'keys.log');
	writeFileSync(log, '');
	const source = `
const {appendFileSync} = require('node:fs');
const log = ${JSON.stringify(log)};
const typed = ${JSON.stringify(typed)};
const transitions = ${JSON.stringify(inputs.transitions?.map(frame) ?? [])};
process.on('SIGUSR2', () => { const next = transitions.shift(); if (next) process.stdout.write('TUI_GRID BYTES ' + next + '\\n'); });
process.stdout.write('TUI_GRID READY 120 36 STREAM\\n');
process.stdout.write('TUI_GRID BYTES ' + ${JSON.stringify(frame(inputs.initial))} + '\\n');
let pending = '';
process.stdin.on('data', (chunk) => {
  pending += chunk.toString();
  while (pending.includes('\\n')) {
    const line = pending.slice(0, pending.indexOf('\\n'));
    pending = pending.slice(pending.indexOf('\\n') + 1);
    const [command, payload] = line.split(' ');
    if (command === 'write') {
      const key = Buffer.from(payload, 'hex').toString();
      appendFileSync(log, JSON.stringify(key) + '\\n');
      if (key === '\\x1b') process.stdout.write('TUI_GRID BYTES ' + ${JSON.stringify(frame(inputs.afterEscape))} + '\\n');
      if (key === typed) process.stdout.write('TUI_GRID BYTES ' + ${JSON.stringify(frame(composed))} + '\\n');
    }
    if (command === 'stop' || command === 'finish') {
      process.stdout.write('TUI_GRID FINISHED 0\\n'); process.exit(0);
    }
    process.stdout.write('TUI_GRID OK\\n');
  }
});`;
	const driver = await PtyGridDriver.start({
		repository: root,
		driver: controlledExecutable(root, 'launch-frames', source),
		surface: 'codex-acp',
		keys: 'legacy',
		evidence: root,
	});
	let clock = Date.now();
	if (inputs.expireOnResume || inputs.expireOnReady)
		vi.spyOn(Date, 'now').mockImplementation((): number => clock);
	const seenFrames = new Set<number>();
	const inputDriver = {
		frameGeneration: (): number => driver.frameGeneration(),
		send: (bytes: Uint8Array): Promise<void> => driver.send(bytes),
		waitForGrid: (
			name: string,
			matches: (grid: TerminalGrid) => boolean,
			after?: number,
			bound?: number,
		): Promise<TerminalGrid> =>
			driver.waitForGrid(
				name,
				(grid) => {
					const generation = driver.frameGeneration();
					const isResuming = gridLines(grid).some((line) => line.includes('Resuming session…'));
					if (isResuming && !seenFrames.has(generation)) {
						seenFrames.add(generation);
						if (inputs.expireOnResume) clock += 8001;
						else if (inputs.transitions?.length) process.kill(driver.processId(), 'SIGUSR2');
					}
					if (
						inputs.expireOnReady &&
						!isResuming &&
						gridLines(grid).some((line) => line.startsWith('› Ask Codex'))
					)
						clock += 8001;
					return matches(grid);
				},
				after,
				bound,
			),
	};
	const captured: { scene: string; lines: readonly string[] }[] = [];
	return {
		// The real journey's first input suffices to expose a startup-key hazard; later provider
		// effects are outside this controlled terminal's contract.
		drive: (): Promise<string[]> =>
			driveCodexJourney(
				inputDriver,
				{ ...journey, steps: [first] },
				'legacy',
				async (step, grid): Promise<void> => {
					captured.push({ scene: step.checkpoint.scene, lines: gridLines(grid) });
				},
				null,
			),
		sent: (): string[] =>
			readFileSync(log, 'utf8')
				.trim()
				.split('\n')
				.filter(Boolean)
				.map((line) => z.string().parse(JSON.parse(line))),
		captured,
		close: async (): Promise<void> => {
			await driver.stop();
		},
	};
}

for (const kind of ['prompt', 'skill'] as const) {
	test(`${kind}: update modal then composer sends exactly one Escape before typing and retains frames`, async () => {
		const fixture = await launchFrameCase({ kind, initial: UPDATE, afterEscape: COMPOSER });
		try {
			await fixture.drive();
			expect(fixture.sent()).toHaveLength(2);
			expect(fixture.sent()[0]).toBe('\x1b');
			expect(fixture.sent()[1]).toMatch(kind === 'prompt' ? /^Reply with/u : /^\$bridge_probe/u);
			expect(fixture.captured.map((entry) => entry.scene)).toEqual([
				'startup-update-modal',
				'startup-composer-ready',
				kind === 'prompt' ? 'prompt-composed' : 'skill-picker-open-1',
			]);
			expect(fixture.captured[0]?.lines.join('\n')).toContain('Update available');
			expect(fixture.captured[1]?.lines.join('\n')).toContain('Ask Codex to do anything');
		} finally {
			await fixture.close();
		}
	});
	test(`${kind}: modal surviving Escape fails without typing or further keys`, async () => {
		const fixture = await launchFrameCase({ kind, initial: UPDATE, afterEscape: UPDATE });
		try {
			await expect(fixture.drive()).rejects.toThrow('composer is not ready after one Escape');
			expect(fixture.sent()).toEqual(['\x1b']);
		} finally {
			await fixture.close();
		}
	});
	test(`${kind}: ready composer sends no Escape`, async () => {
		const fixture = await launchFrameCase({ kind, initial: COMPOSER, afterEscape: COMPOSER });
		try {
			await fixture.drive();
			expect(fixture.sent()).toHaveLength(1);
			expect(fixture.sent()).not.toContain('\x1b');
			expect(fixture.captured[0]?.scene).toBe('startup-composer-ready');
		} finally {
			await fixture.close();
		}
	});
	test(`${kind}: unknown non-composer frame sends no keys`, async () => {
		const fixture = await launchFrameCase({
			kind,
			initial: ['Unexpected startup dialog'],
			afterEscape: COMPOSER,
		});
		try {
			await expect(fixture.drive()).rejects.toThrow('composer is not ready');
			expect(fixture.sent()).toEqual([]);
		} finally {
			await fixture.close();
		}
	});
}

for (const kind of ['prompt', 'skill'] as const) {
	for (const [label, initial, afterEscape, transitions, expectedEscapes] of [
		['Escape then Resuming then ready', UPDATE, RESUMING, [COMPOSER], 1],
		['no-modal Resuming then ready', RESUMING, COMPOSER, [COMPOSER], 0],
		['Resuming then modal then ready', RESUMING, COMPOSER, [UPDATE], 1],
	] as const) {
		test(`${kind}: ${label} waits for focus then types once`, async () => {
			const fixture = await launchFrameCase({ kind, initial, afterEscape, transitions });
			try {
				await fixture.drive();
				expect(fixture.sent().filter((key) => key === '\x1b')).toHaveLength(expectedEscapes);
				expect(fixture.sent().filter((key) => key !== '\x1b')).toHaveLength(1);
				expect(fixture.captured.some((entry) => entry.scene === 'startup-composer-ready')).toBe(
					true,
				);
			} finally {
				await fixture.close();
			}
		});
	}
	test(`${kind}: Resuming forever expires the one bound with nothing typed and retains the last frame`, async () => {
		const fixture = await launchFrameCase({
			kind,
			initial: RESUMING,
			afterEscape: RESUMING,
			expireOnResume: true,
		});
		try {
			await expect(fixture.drive()).rejects.toThrow('startup composer readiness');
			expect(fixture.sent()).toEqual([]);
			expect(fixture.captured.at(-1)?.scene).toBe('startup-composer-refused');
			expect(fixture.captured.at(-1)?.lines.join('\n')).toContain('Resuming session…');
		} finally {
			await fixture.close();
		}
	});
	test(`${kind}: Resuming then an error refuses without keys`, async () => {
		const fixture = await launchFrameCase({
			kind,
			initial: RESUMING,
			afterEscape: COMPOSER,
			transitions: [['■ Failed to resume session']],
		});
		try {
			await expect(fixture.drive()).rejects.toThrow('composer is not ready');
			expect(fixture.sent()).toEqual([]);
			expect(fixture.captured.at(-1)?.lines.join('\n')).toContain('■ Failed to resume session');
		} finally {
			await fixture.close();
		}
	});
	for (const afterEscape of [false, true]) {
		test(`${kind}: memories dialog over composer refuses (after Escape=${afterEscape})`, async () => {
			const fixture = await launchFrameCase({
				kind,
				initial: afterEscape ? UPDATE : MEMORY_DIALOG,
				afterEscape: MEMORY_DIALOG,
			});
			try {
				await expect(fixture.drive()).rejects.toThrow('composer is not ready');
				expect(fixture.sent()).toEqual(afterEscape ? ['\x1b'] : []);
			} finally {
				await fixture.close();
			}
		});
	}
}

// The nonmodal update banner is retained history in the mcf38 ready frame. Focus remains on
// the empty composer at row 21, column 2; banner copy alone cannot make it an active dialog.
const MCF38_READY = [
	'╭─────────────────────────────────────────────────────────────────────────────────────────────────────╮',
	'│ ✨ Update available! 0.165.0 -> 0.166.0                                                             │',
	"│ Run sh -c 'curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_NON_INTERACTIVE=1 sh' to update. │",
	'│                                                                                                     │',
	'│ See full release notes:                                                                             │',
	'│ https://github.com/openai/codex/releases/latest                                                     │',
	'╰─────────────────────────────────────────────────────────────────────────────────────────────────────╯',
	'',
	'',
	'  >_ OpenAI Codex (v0.165.0)',
	'     /private/tmp/2026-11-mcf38-host/agent-workspace',
	'',
	'  Tip: Use the desktop app. Run codex app to open it. It installs automatically if needed.',
	'',
	'',
	'› Multiply 12345 by 6789. Reply with only the integer. Do not use tools.',
	'',
	'',
	'• 83810205',
	'',
	'',
	'› Ask Codex to do anything',
	'',
	'  grok-4.7 low',
	'',
	'',
	'',
	'',
	'',
	'',
	'',
	'',
	'',
	'',
	'',
	'',
];
for (const kind of ['prompt', 'skill'] as const) {
	test(`${kind}: mcf38 focused ready frame admits despite the old nonmodal update banner`, async () => {
		const fixture = await launchFrameCase({ kind, initial: MCF38_READY, afterEscape: COMPOSER });
		try {
			await fixture.drive();
			expect(fixture.sent()).toHaveLength(1);
			expect(fixture.sent()).not.toContain('\x1b');
		} finally {
			await fixture.close();
		}
	});
	test(`${kind}: an unknown dialog cannot borrow the observed Resuming label`, async () => {
		const fixture = await launchFrameCase({
			kind,
			initial: [...RESUMING, 'Unexpected startup dialog'],
			afterEscape: COMPOSER,
		});
		try {
			await expect(fixture.drive()).rejects.toThrow('composer is not ready');
			expect(fixture.sent()).toEqual([]);
		} finally {
			await fixture.close();
		}
	});
}

for (const kind of ['prompt', 'skill'] as const) {
	test(`${kind}: a focused composer arriving after the shared bound cannot authorize typing`, async () => {
		const fixture = await launchFrameCase({
			kind,
			initial: COMPOSER,
			afterEscape: COMPOSER,
			expireOnReady: true,
		});
		try {
			await expect(fixture.drive()).rejects.toThrow('startup composer readiness');
			expect(fixture.sent()).toEqual([]);
			expect(fixture.captured.at(-1)?.scene).toBe('startup-composer-refused');
		} finally {
			await fixture.close();
		}
	});
}
