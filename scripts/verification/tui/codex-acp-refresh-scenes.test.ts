import { expect, test } from 'vitest';

import { codexOrdinaryPromptSchema, refreshJourney } from './codex-acp-refresh-scenes.ts';
import type { RefreshJourneyInput } from './codex-acp-refresh-scenes.ts';
import { gridShowsCheckpoint } from './codex-acp-scenes.ts';
import type { CodexCheckpoint } from './codex-acp-scenes.ts';

const INPUT: RefreshJourneyInput = {
	provider: 'claude',
	prompt: 'count slowly from one to forty, then say the closing word',
	completion: 'REFRESH_DONE_4K7Q',
	replyDeadlineMs: 120_000,
};

// Literal Codex 0.165 frames (slash_command.rs:117; chatwidget/skills.rs:31-59;
// skills_toggle_view.rs:66-71 and skills_toggle_basic.snap), never computed by the code under test.
const WORKING = '• Working (4s • esc to interrupt)';
const COMPOSED = `› ${INPUT.prompt}`;
const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;
const SKILLS_ROW = '› /skills  use skills to improve how Codex performs specific tasks';
const MENU = [
	'  Skills',
	'  Choose an action',
	'',
	'› 1. List skills             Tip: press $ to open this list directly',
	'  2. Enable/Disable Skills   Enable or disable skills',
	'',
	'  Press enter to confirm or esc to go back',
];
const MENU_ON_MANAGE = [
	'  Skills',
	'  Choose an action',
	'',
	'  1. List skills             Tip: press $ to open this list directly',
	'› 2. Enable/Disable Skills   Enable or disable skills',
];
const MANAGE = [
	'  Enable/Disable Skills',
	'  Turn skills on or off. Your changes are saved automatically.',
	'',
	'  Type to search skills',
	'› [x] bridge_probe_claude_01a18013  Emit this verification skill',
	'',
	'  space/enter toggle · esc close',
];
const PROGRESS = '• counting: one, two, three';
const REPLY = `• ${INPUT.completion}`;

function scene(name: string): CodexCheckpoint {
	const step = refreshJourney(INPUT).steps.find((candidate) => candidate.checkpoint.scene === name);
	if (step === undefined) throw new Error(`missing scene ${name}`);
	return step.checkpoint;
}
function rejects(name: string, screens: readonly (readonly string[])[]): void {
	for (const screen of screens)
		expect(gridShowsCheckpoint(screen, scene(name)), `${name}: ${screen.join('|')}`).toBe(false);
}

test('refresh sends exactly the prompt, Enter, /skills, Enter, Down, Enter, Esc and no other key', () => {
	const journey = refreshJourney(INPUT);
	expect(journey.name).toBe('codex-acp-j3-claude-refresh');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'refresh-prompt-composed',
		'refresh-turn-running',
		'refresh-skills-command-ready',
		'refresh-skills-menu-open',
		'refresh-manage-selected',
		'refresh-manage-open',
		'refresh-manage-closed',
		'refresh-reply-completed',
	]);
	// No toggle key (Space or Enter in the manage view), no second submit and no interrupt key.
	expect(journey.steps.map((step) => step.actions)).toEqual([
		[{ kind: 'text', text: INPUT.prompt }],
		[{ kind: 'key', key: 'Enter' }],
		[{ kind: 'text', text: '/skills' }],
		[{ kind: 'key', key: 'Enter' }],
		[{ kind: 'key', key: 'Down' }],
		[{ kind: 'key', key: 'Enter' }],
		[{ kind: 'key', key: 'Escape' }],
		[],
	]);
	expect(journey.steps.at(-1)?.checkpoint.deadline_ms).toBe(120_000);
});

test('the prompt is composed and running before /skills, and a finished or interrupted turn fails', () => {
	expect(gridShowsCheckpoint([COMPOSED], scene('refresh-prompt-composed'))).toBe(true);
	rejects('refresh-prompt-composed', [
		[`› count slowly from one`],
		[COMPOSED, WORKING],
		// A reply already on screen before this prompt: stale history cannot pass.
		[REPLY, COMPOSED],
		['  Resuming session…', COMPOSED],
	]);
	expect(
		gridShowsCheckpoint([COMPOSED, WORKING, EMPTY_COMPOSER], scene('refresh-turn-running')),
	).toBe(true);
	rejects('refresh-turn-running', [
		[COMPOSED],
		[WORKING, REPLY],
		['■ Conversation interrupted - use /feedback if something went wrong'],
	]);
	const ready = [PROGRESS, WORKING, SKILLS_ROW, '› /skills'];
	expect(gridShowsCheckpoint(ready, scene('refresh-skills-command-ready'))).toBe(true);
	rejects('refresh-skills-command-ready', [
		// The echo alone, another row, the turn already over, or the reply already drawn.
		[WORKING, '› /skills'],
		[WORKING, '› /status  show current session configuration and token usage', '› /skills'],
		[SKILLS_ROW, '› /skills'],
		[REPLY, WORKING, SKILLS_ROW, '› /skills'],
	]);
});

test('the menu and manage view frames need their own rows and reject a finished turn or a missing view', () => {
	expect(gridShowsCheckpoint([PROGRESS, ...MENU], scene('refresh-skills-menu-open'))).toBe(true);
	rejects('refresh-skills-menu-open', [
		// The slash popup is still open: Enter never dispatched /skills.
		[WORKING, SKILLS_ROW, '› /skills'],
		// The menu opened on the second row, or the turn has already replied.
		MENU_ON_MANAGE,
		[REPLY, ...MENU],
	]);
	expect(gridShowsCheckpoint(MENU_ON_MANAGE, scene('refresh-manage-selected'))).toBe(true);
	rejects('refresh-manage-selected', [
		MENU,
		['› 2. Enable/Disable Skills extra', '  Choose an action'],
		[REPLY, ...MENU_ON_MANAGE],
	]);
	expect(gridShowsCheckpoint([PROGRESS, ...MANAGE], scene('refresh-manage-open'))).toBe(true);
	rejects('refresh-manage-open', [
		// The menu still drawn, no skills to manage, or the turn already over.
		[...MENU_ON_MANAGE, ...MANAGE],
		['• No skills available.', EMPTY_COMPOSER],
		[REPLY, ...MANAGE],
		['  Enable/Disable Skills', '  space/enter toggle · esc close'],
	]);
});

test('Esc closes the view with no toggle while the turn still works, then the reply arrives uninterrupted', () => {
	const closed = [PROGRESS, '', WORKING, '', EMPTY_COMPOSER, '  sonnet low'];
	expect(gridShowsCheckpoint(closed, scene('refresh-manage-closed'))).toBe(true);
	rejects('refresh-manage-closed', [
		// The view is still open, or the menu came back.
		[...MANAGE],
		[WORKING, ...MANAGE, EMPTY_COMPOSER],
		[WORKING, ...MENU, EMPTY_COMPOSER],
		// A skill was toggled: Codex draws the count on close.
		['• 0 skills enabled, 1 skills disabled', WORKING, EMPTY_COMPOSER],
		// The turn ended (no status row), finished with its reply, or was interrupted.
		[PROGRESS, EMPTY_COMPOSER],
		[REPLY, WORKING, EMPTY_COMPOSER],
		['■ Conversation interrupted - use /feedback if something went wrong', WORKING, EMPTY_COMPOSER],
		// The composer is not empty and usable.
		[WORKING, '› /skills'],
	]);
	const replied = [PROGRESS, REPLY, '', EMPTY_COMPOSER, '  sonnet low'];
	expect(gridShowsCheckpoint(replied, scene('refresh-reply-completed'))).toBe(true);
	rejects('refresh-reply-completed', [
		// Still working, interrupted, toggled, a partial reply, or the composer busy.
		[PROGRESS, REPLY, WORKING, EMPTY_COMPOSER],
		[REPLY, '■ Conversation interrupted - use /feedback if something went wrong', EMPTY_COMPOSER],
		[REPLY, '• 1 skills enabled, 0 skills disabled', EMPTY_COMPOSER],
		['• REFRESH_DONE', EMPTY_COMPOSER],
		[REPLY, `› ${INPUT.prompt}`],
		[],
	]);
});

test('an ordinary prompt opens no popup or mode, and an echo cannot satisfy the completion', () => {
	expect(codexOrdinaryPromptSchema.parse(INPUT.prompt)).toBe(INPUT.prompt);
	for (const prompt of [
		'',
		' padded',
		'padded ',
		'/skills',
		'!ls',
		'use $bridge_probe_claude_01a18013',
		'see @README.md',
		'line\nbreak',
		'esc\u001b[A',
		'x'.repeat(101),
	])
		expect(() => refreshJourney({ ...INPUT, prompt }), JSON.stringify(prompt)).toThrow();
	for (const completion of [
		// In the prompt, or text Codex itself draws on these frames.
		'count slowly',
		'Enable/Disable Skills',
		'Ask Codex to do anything',
		// Too short to be distinctive, or more than one line.
		'DONE',
		'REFRESH\nDONE_4K7Q',
	])
		expect(() => refreshJourney({ ...INPUT, completion }), JSON.stringify(completion)).toThrow();
});
