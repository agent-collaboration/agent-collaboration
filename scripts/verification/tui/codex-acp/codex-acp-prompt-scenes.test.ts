import { expect, test } from 'vitest';

import { codexOrdinaryPromptSchema, promptJourney } from './codex-acp-prompt-scenes.ts';
import type { PromptJourneyInput } from './codex-acp-prompt-scenes.ts';
import { gridShowsCheckpoint } from './codex-acp-scenes.ts';
import type { CodexCheckpoint } from './codex-acp-scenes.ts';

// An arithmetic control: the expected integer is never typed.
const INPUT: PromptJourneyInput = {
	provider: 'claude',
	prompt: 'Reply with only the product of 4321 and 5678, digits only',
	completion: '24534638',
	replyDeadlineMs: 120_000,
};

// Literal Codex 0.165 frames (chatwidget.rs:2101; history_cell/notices.rs:331-336), never computed.
const COMPOSED = `› ${INPUT.prompt}`;
const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;
const STATUS = '  haiku low';
const WORKING = '• Working (2s • esc to interrupt)';
const REPLY = `• ${INPUT.completion}`;

function scenes(): { readonly composed: CodexCheckpoint; readonly replied: CodexCheckpoint } {
	const [composed, replied] = promptJourney(INPUT).steps;
	if (composed === undefined || replied === undefined) throw new Error('prompt scenes missing');
	return { composed: composed.checkpoint, replied: replied.checkpoint };
}

test('the prompt journey types only the prompt, then one Enter, and nothing else', () => {
	const journey = promptJourney(INPUT);
	expect(journey.name).toBe('codex-acp-j2-claude-prompt');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'prompt-composed',
		'prompt-replied',
	]);
	expect(journey.steps.map((step) => step.actions)).toEqual([
		[{ kind: 'text', text: INPUT.prompt }],
		[{ kind: 'key', key: 'Enter' }],
	]);
	expect(journey.steps.at(-1)?.checkpoint.deadline_ms).toBe(120_000);
});

test('Enter waits for the exact prompt drawn in an idle, resumed composer with no stale reply or error', () => {
	const { composed } = scenes();
	expect(gridShowsCheckpoint(['• an earlier reply', '', COMPOSED, '', STATUS], composed)).toBe(
		true,
	);
	for (const screen of [
		// The prompt text outside a composer line.
		[`• ${INPUT.prompt}`, EMPTY_COMPOSER],
		// Typed text still buffered, a truncated prompt, or a blank screen.
		[EMPTY_COMPOSER, STATUS],
		['› Reply with only the product of 4321'],
		[],
		// The completion already on screen: a stale reply cannot pass.
		[REPLY, COMPOSED],
		// An earlier error cell, a running turn, or the resume still pending.
		['■ Failed to load hooks: hooks/list failed in TUI', COMPOSED],
		[WORKING, COMPOSED],
		['  Resuming session…', COMPOSED],
	])
		expect(gridShowsCheckpoint(screen, composed), screen.join('|')).toBe(false);
});

test('the reply frame needs the completion, an empty composer and no status row or new error', () => {
	const { replied } = scenes();
	expect(gridShowsCheckpoint([COMPOSED, REPLY, '', EMPTY_COMPOSER, STATUS], replied)).toBe(true);
	// A fast reply never drew a Working frame; that is not required.
	expect(gridShowsCheckpoint([REPLY, EMPTY_COMPOSER], replied)).toBe(true);
	for (const screen of [
		// The CLI exited or drew nothing, or only the echoed prompt.
		[],
		[COMPOSED],
		// A partial or wrong answer.
		['• 2453463', EMPTY_COMPOSER],
		['• 24534639', EMPTY_COMPOSER],
		// Still working, interrupted, or a new error.
		[REPLY, WORKING, EMPTY_COMPOSER],
		[REPLY, '■ Conversation interrupted - use /feedback if something went wrong', EMPTY_COMPOSER],
		[REPLY, '■ stream disconnected before completion', EMPTY_COMPOSER],
		// The composer is busy: the prompt never left it, or more text was typed.
		[REPLY, COMPOSED],
		[REPLY, '› and one more thing'],
	])
		expect(gridShowsCheckpoint(screen, replied), screen.join('|')).toBe(false);
});

test('only an ordinary prompt and a distinctive, untyped completion make a journey', () => {
	expect(codexOrdinaryPromptSchema.parse(INPUT.prompt)).toBe(INPUT.prompt);
	for (const prompt of [
		'',
		' padded',
		'padded ',
		'/model',
		'!echo 24534638',
		'use $bridge_probe_claude_01a18013',
		'see @probe.png',
		'two\nlines',
		'x'.repeat(101),
	])
		expect(() => promptJourney({ ...INPUT, prompt }), JSON.stringify(prompt)).toThrow();
	for (const completion of [
		// Typed in the prompt, drawn by Codex, too short to be distinctive, or more than one line.
		'4321 and 5678',
		'Ask Codex to do anything',
		'2453',
		'24534638\nmore',
	])
		expect(() => promptJourney({ ...INPUT, completion }), JSON.stringify(completion)).toThrow();
});
