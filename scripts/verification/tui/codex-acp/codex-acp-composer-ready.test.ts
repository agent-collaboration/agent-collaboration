/** Real streaming frame/driver boundary; the child is a controlled terminal, never Codex. */
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';
import { z } from 'zod';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import { promptJourney } from './codex-acp-prompt-scenes.ts';
import { driveCodexJourney } from './codex-acp-run.ts';
import { skillJourney } from './codex-acp-scenes.ts';

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
	return Buffer.from(`\u001b[?2026h\u001b[2J\u001b[H${rows.join('\r\n')}\u001b[?2026l`).toString(
		'hex',
	);
}

async function launchFrameCase(inputs: {
	readonly kind: 'prompt' | 'skill';
	readonly initial: readonly string[];
	readonly afterEscape: readonly string[];
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
	const captured: { scene: string; lines: readonly string[] }[] = [];
	return {
		// The real journey's first input suffices to expose a startup-key hazard; later provider
		// effects are outside this controlled terminal's contract.
		drive: (): Promise<string[]> =>
			driveCodexJourney(
				driver,
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
