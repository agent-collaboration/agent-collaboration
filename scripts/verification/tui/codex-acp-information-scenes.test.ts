import { expect, test } from 'vitest';

import {
	codexInformationCommandSchema,
	informationJourney,
} from './codex-acp-information-scenes.ts';
import type { CodexInformationCommand } from './codex-acp-information-scenes.ts';
import { gridShowsCheckpoint, gridShowsErrorLine } from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

type CodexStep = CodexJourney['steps'][number];

// Literal Codex 0.165 text (slash_command.rs:119,156; chatwidget/hooks.rs:30 with hooks_rpc.rs:35;
// app/background_requests.rs:746 with :803), never computed by the code under test.
const HOOKS_ROW = '› /hooks  view and manage lifecycle hooks';
const MCP_ROW = '› /mcp  list MCP tools; use /mcp verbose or /mcp login <name>';
const HOOKS_REFUSAL = '■ Failed to load hooks: hooks/list failed in TUI';
const MCP_REFUSAL = '■ Failed to load MCP inventory: mcpServerStatus/list failed in TUI';
const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;
const STATUS = '  sonnet low';

function scenes(command: CodexInformationCommand): {
	readonly journey: CodexJourney;
	readonly ready: CodexStep;
	readonly refused: CodexStep;
} {
	const journey = informationJourney('claude', command);
	const [ready, refused] = journey.steps;
	if (ready === undefined || refused === undefined || journey.steps.length !== 2)
		throw new Error('information scenes missing');
	return { journey, ready, refused };
}

test('each information command types only its own name, waits for its row, then presses Enter', () => {
	for (const [command, name] of [
		['hooks', 'codex-acp-j2-claude-hooks'],
		['mcp', 'codex-acp-j2-claude-mcp'],
	] as const) {
		const { journey, ready, refused } = scenes(command);
		expect(journey.name).toBe(name);
		expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
			`${command}-command-ready`,
			`${command}-refused`,
		]);
		expect(ready.actions).toEqual([{ kind: 'text', text: `/${command}` }]);
		expect(refused.actions).toEqual([{ kind: 'key', key: 'Enter' }]);
	}
	// No arbitrary slash text, argument, login or option is an information command.
	for (const command of ['login', 'mcp verbose', 'mcp login fixture', 'model', 'hooks ', '/hooks'])
		expect(codexInformationCommandSchema.safeParse(command).success, command).toBe(false);
});

test('the ready frame needs the selected command row and rejects echo, another row and a stale error', () => {
	const hooks = scenes('hooks').ready.checkpoint;
	const mcp = scenes('mcp').ready.checkpoint;
	expect(gridShowsCheckpoint([HOOKS_ROW, '', '› /hooks', STATUS], hooks)).toBe(true);
	expect(gridShowsCheckpoint([MCP_ROW, '', '› /mcp', STATUS], mcp)).toBe(true);
	for (const [checkpoint, screen] of [
		// The composer echo alone: the typed text has not reached the popup.
		[hooks, ['› /hooks', STATUS]],
		// Another command's row, an unselected row, or the description on a different command.
		[hooks, [MCP_ROW, '› /hooks']],
		[hooks, ['  /hooks  view and manage lifecycle hooks', '› /hooks']],
		[hooks, ['› /hookz  view and manage lifecycle hooks', '› /hooks']],
		[mcp, ['› /mcp  list MCP tools', '› /mcp']],
		// An error cell left by earlier input in this TUI.
		[hooks, [HOOKS_REFUSAL, HOOKS_ROW, '› /hooks']],
		[mcp, [MCP_REFUSAL, MCP_ROW, '› /mcp']],
		// Typed during the startup draft, before the resume lands.
		[hooks, ['  Resuming session…', HOOKS_ROW, '› /hooks']],
	] as const)
		expect(gridShowsCheckpoint(screen, checkpoint), screen.join('|')).toBe(false);
});

test('the refused frame needs one new whole error line, no open popup and an empty composer', () => {
	const hooks = scenes('hooks').refused.checkpoint;
	const mcp = scenes('mcp').refused.checkpoint;
	expect(
		gridShowsCheckpoint(['• earlier reply', HOOKS_REFUSAL, '', EMPTY_COMPOSER, STATUS], hooks),
	).toBe(true);
	expect(gridShowsCheckpoint([MCP_REFUSAL, '', EMPTY_COMPOSER, STATUS], mcp)).toBe(true);
	for (const [checkpoint, screen] of [
		// The CLI exited or drew nothing.
		[hooks, []],
		[hooks, ['', '']],
		// An unrelated error, the other command's error, or the text outside an error cell.
		[hooks, ['■ Failed to load plugins: plugin/list failed in TUI', EMPTY_COMPOSER]],
		[hooks, [MCP_REFUSAL, EMPTY_COMPOSER]],
		[hooks, ['• Failed to load hooks: hooks/list failed in TUI', EMPTY_COMPOSER]],
		[hooks, [`  ${HOOKS_REFUSAL}`, EMPTY_COMPOSER]],
		// A different message after the same leading text.
		[hooks, [`${HOOKS_REFUSAL}: -32000 Hook discovery is not supported`, EMPTY_COMPOSER]],
		// The popup is still open, or the command is still in the composer.
		[hooks, [HOOKS_REFUSAL, HOOKS_ROW, '› /hooks']],
		[hooks, [HOOKS_REFUSAL, '› /hooks']],
		// The MCP request is still pending.
		[mcp, ['• Loading MCP inventory', MCP_REFUSAL, EMPTY_COMPOSER]],
		// The hooks browser opened instead: no refusal at all.
		[hooks, ['  Hooks', '  No hooks configured', EMPTY_COMPOSER]],
	] as const)
		expect(gridShowsCheckpoint(screen, checkpoint), screen.join('|')).toBe(false);
});

test('an error line is a whole history cell starting at the line start', () => {
	expect(
		gridShowsErrorLine([`${HOOKS_REFUSAL}   `], 'Failed to load hooks: hooks/list failed in TUI'),
	).toBe(true);
	for (const line of [
		`${HOOKS_REFUSAL} extra`,
		` ${HOOKS_REFUSAL}`,
		'Failed to load hooks: hooks/list failed in TUI',
		'■ Failed to load hooks: hooks/list failed',
	])
		expect(gridShowsErrorLine([line], 'Failed to load hooks: hooks/list failed in TUI'), line).toBe(
			false,
		);
	// Both refusals fit one 120-column row, so a whole-line match can be drawn at all.
	for (const line of [HOOKS_REFUSAL, MCP_REFUSAL]) expect(line.length).toBeLessThan(120);
});
