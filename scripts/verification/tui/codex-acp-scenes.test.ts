import { expect, test } from 'vitest';

import {
	CODEX_0_165_TEXT,
	compactJourney,
	composedSkillText,
	encodeCodexAction,
	codexSkillDisplayName,
	gridShowsCheckpoint,
	interruptJourney,
	modelJourney,
	pickerOnlyJourney,
	reopenJourney,
	requireCheckpointSequence,
	skillJourney,
} from './codex-acp-scenes.ts';
import type { CodexCheckpoint, SkillJourneyInput } from './codex-acp-scenes.ts';

const CURSOR_INPUT: SkillJourneyInput = {
	provider: 'cursor',
	skills: ['bridge_probe_cursor_a_01a18013', 'bridge_probe_cursor_b_01a18013'],
	prefix: 'please ',
	args: 'ALPHA',
	// Each selected fixture skill replies "<its own prefix>|<argument>|<image colour>".
	expectedEffects: ['ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED', 'ACP_BRIDGE_CURSOR_B_5D820A|ALPHA|RED'],
	imagePath: '/task/probe.png',
	effectDeadlineMs: 120_000,
};
const CLAUDE_INPUT: SkillJourneyInput = {
	...CURSOR_INPUT,
	provider: 'claude',
	skills: ['bridge_probe_claude_01a18013'],
	prefix: '',
	expectedEffects: ['ACP_BRIDGE_CLAUDE_7FA28C|ALPHA|RED'],
};
function checkpoint(journey: ReturnType<typeof skillJourney>, scene: string): CodexCheckpoint {
	const step = journey.steps.find((candidate) => candidate.checkpoint.scene === scene);
	if (step === undefined) throw new Error(`missing scene ${scene}`);
	return step.checkpoint;
}

test('J1 selects every skill through the picker in order, then composes and submits', () => {
	const journey = skillJourney(CURSOR_INPUT);
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'skill-picker-open-1',
		'skill-selected-1',
		'skill-picker-open-2',
		'skill-selected-2',
		'skill-input-composed',
		'skill-effect',
	]);
	expect(composedSkillText(CURSOR_INPUT)).toBe(
		'please $bridge_probe_cursor_a_01a18013 $bridge_probe_cursor_b_01a18013 ALPHA',
	);
	expect(checkpoint(journey, 'skill-input-composed').require).toEqual([
		'please $bridge_probe_cursor_a_01a18013 $bridge_probe_cursor_b_01a18013 ALPHA',
		'[Image #1]',
	]);
	expect(checkpoint(journey, 'skill-effect')).toEqual({
		scene: 'skill-effect',
		require: ['ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED', 'ACP_BRIDGE_CURSOR_B_5D820A|ALPHA|RED'],
		forbid: ['■ '],
		deadline_ms: 120_000,
	});
	expect(journey.steps.flatMap((step) => step.actions)).toEqual([
		{ kind: 'text', text: 'please $bridge_probe_cursor_a_01a18013' },
		{ kind: 'key', key: 'Tab' },
		{ kind: 'text', text: '$bridge_probe_cursor_b_01a18013' },
		{ kind: 'key', key: 'Tab' },
		{ kind: 'text', text: 'ALPHA' },
		{ kind: 'paste', text: '/task/probe.png' },
		{ kind: 'key', key: 'Enter' },
	]);
	expect(skillJourney(CLAUDE_INPUT).steps.map((step) => step.checkpoint.scene)).toEqual([
		'skill-picker-open-1',
		'skill-selected-1',
		'skill-input-composed',
		'skill-effect',
	]);
});

test('J1 refuses echo-satisfiable effects, Claude multi-selection and repeated selections', () => {
	expect(() => skillJourney({ ...CURSOR_INPUT, expectedEffects: ['ALPHA'] })).toThrow(
		'an echo could satisfy it',
	);
	expect(() => skillJourney({ ...CURSOR_INPUT, expectedEffects: ['bridge_probe'] })).toThrow(
		'an echo could satisfy it',
	);
	expect(() => skillJourney({ ...CURSOR_INPUT, expectedEffects: [] })).toThrow(
		'an echo could satisfy it',
	);
	expect(() =>
		skillJourney({ ...CLAUDE_INPUT, skills: ['bridge_probe_claude_01a18013', 'other'] }),
	).toThrow('exactly one selected skill');
	expect(() =>
		skillJourney({
			...CURSOR_INPUT,
			skills: ['bridge_probe_cursor_a_01a18013', 'bridge_probe_cursor_a_01a18013'],
		}),
	).toThrow('distinct selected skills');
});

test('the effect checkpoint needs every selected effect and rejects echo, error, blank and wrong scenes', () => {
	const effect = checkpoint(skillJourney(CURSOR_INPUT), 'skill-effect');
	expect(
		gridShowsCheckpoint(
			['• ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED', '  ACP_BRIDGE_CURSOR_B_5D820A|ALPHA|RED'],
			effect,
		),
	).toBe(true);
	// Only the first selection ran: a first-only drop must fail.
	expect(gridShowsCheckpoint(['• ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED'], effect)).toBe(false);
	// A wrong image colour, the echoed input, an error, a blank screen.
	expect(
		gridShowsCheckpoint(
			['• ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|BLUE', 'ACP_BRIDGE_CURSOR_B_5D820A|ALPHA|BLUE'],
			effect,
		),
	).toBe(false);
	expect(gridShowsCheckpoint([composedSkillText(CURSOR_INPUT)], effect)).toBe(false);
	expect(
		gridShowsCheckpoint(
			['• ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED ACP_BRIDGE_CURSOR_B_5D820A|ALPHA|RED', '■ x'],
			effect,
		),
	).toBe(false);
	expect(gridShowsCheckpoint(['', '', ''], effect)).toBe(false);
});

// The slash popup row for a command (slash_command.rs:99,135), distinct from the composer echo.
const COMPACT_ROW = '› /compact    summarize conversation to prevent hitting the context limit';
const MODEL_ROW = '› /model      choose what model and reasoning effort to use';

test('J2 Enter follows only the drawn command row, never the composer echo alone', () => {
	for (const [journey, row, echo] of [
		[compactJourney('cursor', 120_000), COMPACT_ROW, '› /compact'],
		[modelJourney('cursor', [], 'session', ['Model changed'], 8_000), MODEL_ROW, '› /model'],
	] as const) {
		const [ready, submitted] = journey.steps;
		if (ready === undefined || submitted === undefined) throw new Error('command scene missing');
		expect(ready.checkpoint.scene).toMatch(/-command-ready$/u);
		expect(ready.actions.every((action) => action.kind === 'text')).toBe(true);
		expect(submitted.actions[0]).toEqual({ kind: 'key', key: 'Enter' });
		expect(gridShowsCheckpoint([echo, row], ready.checkpoint)).toBe(true);
		expect(gridShowsCheckpoint([echo], ready.checkpoint)).toBe(false);
	}
});

test('J2 compaction succeeds only on Claude and Cursor must show an error without a compaction', () => {
	const claude = compactJourney('claude', 120_000).steps[1]?.checkpoint;
	const cursor = compactJourney('cursor', 120_000).steps[1]?.checkpoint;
	if (claude === undefined || cursor === undefined) throw new Error('compaction scene missing');
	expect(gridShowsCheckpoint(['• Context compacted · 3s'], claude)).toBe(true);
	expect(gridShowsCheckpoint(['• Compacted the conversation.'], claude)).toBe(false);
	expect(gridShowsCheckpoint(['■ compact is not supported'], cursor)).toBe(true);
	expect(gridShowsCheckpoint(['■ x', 'Context compacted'], cursor)).toBe(false);
	expect(cursor.deadline_ms).toBe(8_000);
});

test('J3 needs a running turn before its interruption', () => {
	const [composed, running, interrupted] = interruptJourney(
		'claude',
		'count slowly to fifty',
	).steps;
	if (composed === undefined || running === undefined || interrupted === undefined)
		throw new Error('interrupt scene missing');
	// The prompt is drawn in the composer, and not yet running, before its Enter.
	expect(composed.actions).toEqual([{ kind: 'text', text: 'count slowly to fifty' }]);
	expect(gridShowsCheckpoint(['› count slowly to fifty'], composed.checkpoint)).toBe(true);
	expect(gridShowsCheckpoint(['› count slowly'], composed.checkpoint)).toBe(false);
	expect(
		gridShowsCheckpoint(['› count slowly to fifty', '• Working (0s)'], composed.checkpoint),
	).toBe(false);
	expect(running.actions).toEqual([{ kind: 'key', key: 'Enter' }]);
	expect(gridShowsCheckpoint(['• Working (3s • esc to interrupt)'], running.checkpoint)).toBe(true);
	expect(
		gridShowsCheckpoint(
			['■ Conversation interrupted - use /feedback if something went wrong'],
			interrupted.checkpoint,
		),
	).toBe(true);
	expect(interrupted.actions).toEqual([{ kind: 'key', key: 'Escape' }]);
});

test('missing, reordered or extra checkpoints are rejected', () => {
	const journey = skillJourney(CURSOR_INPUT);
	const scenes = journey.steps.map((step) => step.checkpoint.scene);
	requireCheckpointSequence(scenes, journey);
	for (const observed of [
		scenes.slice(1),
		[scenes[1] ?? '', scenes[0] ?? '', ...scenes.slice(2)],
		[...scenes, 'skill-effect'],
	])
		expect(() => requireCheckpointSequence(observed, journey)).toThrow(
			'checkpoint sequence differs',
		);
});

test('actions encode as the exact PTY bytes for each key protocol', () => {
	expect(encodeCodexAction({ kind: 'text', text: '$é' }, 'legacy')).toEqual(
		Buffer.from([0x24, 0xc3, 0xa9]),
	);
	expect(encodeCodexAction({ kind: 'paste', text: '/a.png' }, 'legacy').toString()).toBe(
		'\u001b[200~/a.png\u001b[201~',
	);
	expect(encodeCodexAction({ kind: 'key', key: 'Ctrl+C' }, 'legacy')).toEqual(Buffer.from([3]));
	expect(encodeCodexAction({ kind: 'key', key: 'Ctrl+C' }, 'kitty').toString()).toBe(
		'\u001b[99;5u',
	);
	expect(encodeCodexAction({ kind: 'key', key: 'Tab' }, 'legacy').toString()).toBe('\t');
});

// Codex 0.165 row text: "› " or two spaces, the display name, padding, then "[Skill]" and the
// description (skill_popup.rs:156-178, selection_row_layout.rs:116-180).
const ROW_A_SELECTED =
	"› bridge_probe_cursor_a_01a...  [Skill] Emit this verification skill's response only when explicitly selected.";
const ROW_B_UNSELECTED =
	"  bridge_probe_cursor_b_01a...  [Skill] Emit this verification skill's response only when explicitly selected.";
const COMPOSER_ECHO = '› $bridge_probe_cursor_a_01a18013';

test('popup display names follow the pinned 28-grapheme rule', () => {
	expect(codexSkillDisplayName('bridge_probe_cursor_a_01a18013')).toBe(
		'bridge_probe_cursor_a_01a...',
	);
	expect(codexSkillDisplayName('bridge_probe_claude_01a18013')).toBe(
		'bridge_probe_claude_01a18013',
	);
	// Graphemes, not code units: a 29-grapheme name with an emoji keeps 25 graphemes.
	expect(codexSkillDisplayName(`📂${'x'.repeat(28)}`)).toBe(`📂${'x'.repeat(24)}...`);
});

test('the picker checkpoint needs the selected skill row, not the composer echo', () => {
	const picker = checkpoint(skillJourney(CURSOR_INPUT), 'skill-picker-open-1');
	expect(picker.selected_skill_row).toBe('bridge_probe_cursor_a_01a...');
	expect(
		gridShowsCheckpoint(
			[ROW_A_SELECTED, ROW_B_UNSELECTED, CODEX_0_165_TEXT.skillPopupHint, COMPOSER_ECHO],
			picker,
		),
	).toBe(true);
	// The retained wrong screen at c4 (picker-no-matches-at-c4.log) must be rejected.
	expect(
		gridShowsCheckpoint(['  no matches', 'enter insert · esc close', COMPOSER_ECHO], picker),
	).toBe(false);
	// The right name only in the composer, an unselected row, a row without the skill tag, the
	// untruncated name the popup never shows, or a blank screen are all rejected.
	expect(gridShowsCheckpoint([CODEX_0_165_TEXT.skillPopupHint, COMPOSER_ECHO], picker)).toBe(false);
	expect(
		gridShowsCheckpoint(
			[
				ROW_A_SELECTED.replace('› ', '  '),
				'› bridge_probe_cursor_b_01a...  [Skill] Emit',
				CODEX_0_165_TEXT.skillPopupHint,
			],
			picker,
		),
	).toBe(false);
	expect(
		gridShowsCheckpoint(
			['› bridge_probe_cursor_a_01a...  Emit', CODEX_0_165_TEXT.skillPopupHint],
			picker,
		),
	).toBe(false);
	expect(
		gridShowsCheckpoint(
			['› bridge_probe_cursor_a_01a18013  [Skill] Emit', CODEX_0_165_TEXT.skillPopupHint],
			picker,
		),
	).toBe(false);
	expect(gridShowsCheckpoint(['', '', ''], picker)).toBe(false);
});

test('the picker-only journey opens the selected row and clears the draft with no model input', () => {
	const journey = pickerOnlyJourney('claude', 'bridge_probe_claude_01a18013');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'skill-picker-open-1',
		'draft-cleared',
	]);
	// Nothing is submitted: no Enter, no Tab into the composer, no pasted image.
	expect(journey.steps.flatMap((step) => step.actions)).toEqual([
		{ kind: 'text', text: '$bridge_probe_claude_01a18013' },
		{ kind: 'key', key: 'Ctrl+C' },
	]);
	const [picker, cleared] = journey.steps;
	if (picker === undefined || cleared === undefined) throw new Error('picker scene missing');
	const claudeRow = '› bridge_probe_claude_01a18013  [Skill] Emit';
	expect(gridShowsCheckpoint([claudeRow, CODEX_0_165_TEXT.skillPopupHint], picker.checkpoint)).toBe(
		true,
	);
	expect(
		gridShowsCheckpoint(
			['› $bridge_probe_claude_01a18013', CODEX_0_165_TEXT.skillPopupHint],
			picker.checkpoint,
		),
	).toBe(false);
	// The Claude 828 screen after Ctrl+C: history, the empty composer, the status line, no hint.
	const reply = '• ACP_BRIDGE_CLAUDE_7FA28C|ALPHA|RED';
	const emptyComposer = `› Ask Codex to do anything${' '.repeat(94)}`;
	expect(
		gridShowsCheckpoint([reply, '', emptyComposer, '', '  Opus 5.5 high'], cleared.checkpoint),
	).toBe(true);
	for (const screen of [
		// The 828 picker frame itself: popup open over a non-empty draft.
		[reply, claudeRow, CODEX_0_165_TEXT.skillPopupHint, '› $bridge_probe_claude_01a18013'],
		// The draft is still there, or typed text merely begins with the placeholder words.
		['› $bridge_probe_claude_01a18013'],
		['› Ask Codex to do anything else'],
		// The placeholder words only in history, above a non-empty composer.
		['• Ask Codex to do anything', '› $bridge_probe_claude_01a18013'],
		// An empty composer while the popup hint is still drawn.
		[emptyComposer, CODEX_0_165_TEXT.skillPopupHint],
		// A side conversation's placeholder (chatwidget.rs:2102) is not this composer.
		['› Ask a follow-up question'],
	])
		expect(gridShowsCheckpoint(screen, cleared.checkpoint), screen.join('|')).toBe(false);
});

test('reopen replays every declared prior reply line without new input', () => {
	const journey = reopenJourney('cursor', [
		'ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED',
		'ACP_BRIDGE_CURSOR_B_5D820A|ALPHA|RED',
	]);
	const [replayed] = journey.steps;
	if (replayed === undefined || journey.steps.length !== 1) throw new Error('reopen scene');
	expect(replayed.actions).toEqual([]);
	expect(
		gridShowsCheckpoint(
			[
				'• ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED ACP_BRIDGE_CURSOR_B_5D820A|ALPHA|RED',
				'■ an earlier error cell',
			],
			replayed.checkpoint,
		),
	).toBe(true);
	expect(gridShowsCheckpoint(['• ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED'], replayed.checkpoint)).toBe(
		false,
	);
	// An empty expectation would pass on any screen.
	expect(() => reopenJourney('cursor', [])).toThrow();
});

// The Router's refusal of a Codex default save (router_session_app_server_methods.rs, cd9668cd5).
const ROUTER_SAVE_REFUSAL =
	'Saving Codex configuration/defaults is not supported on this ACP endpoint';

test('the session-only apply sends one plain s after the declared navigation', () => {
	const navigation = [{ kind: 'key', key: 'Down' }] as const;
	const session = modelJourney(
		'claude',
		navigation,
		'session',
		['Model changed to fixture-model', 'for this session only'],
		8_000,
	);
	expect(session.name).toBe('codex-acp-j2-claude-model-session');
	expect(session.steps.map((step) => step.checkpoint.scene)).toEqual([
		'model-command-ready',
		'model-popup-open',
		'model-applied',
	]);
	const [, popup, applied] = session.steps;
	if (popup === undefined || applied === undefined) throw new Error('model scene missing');
	expect(applied.actions).toEqual([...navigation, { kind: 'text', text: 's' }]);
	expect(encodeCodexAction({ kind: 'text', text: 's' }, 'kitty').toString()).toBe('s');
	// model_popups.rs:186,279: either model popup opens.
	expect(gridShowsCheckpoint(['  Select Model and Effort'], popup.checkpoint)).toBe(true);
	expect(gridShowsCheckpoint(['  Select Model'], popup.checkpoint)).toBe(true);
	expect(gridShowsCheckpoint(['› /model'], popup.checkpoint)).toBe(false);
	// app/model_defaults.rs:103-118: model, optional effort and tier labels, then the suffix.
	expect(
		gridShowsCheckpoint(
			['• Model changed to fixture-model high for this session only'],
			applied.checkpoint,
		),
	).toBe(true);
	// A default save, a popup left open (model or reasoning), or a refusal is not this outcome.
	for (const screen of [
		['• Model changed to fixture-model high for this conversation'],
		['• Model changed to fixture-model high for this session only', '  Select Model and Effort'],
		[
			'• Model changed to fixture-model high for this session only',
			'  Select Reasoning Level for fixture-model',
		],
		['■ Failed to update thread settings: serviceTier is not supported'],
	])
		expect(gridShowsCheckpoint(screen, applied.checkpoint)).toBe(false);
});

test('the default apply accepts only the declared honest refusal of the default save', () => {
	const refused = modelJourney(
		'cursor',
		[],
		'default',
		['Failed to save default model', ROUTER_SAVE_REFUSAL],
		8_000,
	).steps[2];
	if (refused === undefined) throw new Error('model refusal scene missing');
	expect(refused.actions).toEqual([{ kind: 'key', key: 'Enter' }]);
	expect(
		gridShowsCheckpoint(
			[`■ Failed to save default model: ${ROUTER_SAVE_REFUSAL}`],
			refused.checkpoint,
		),
	).toBe(true);
	expect(
		gridShowsCheckpoint(['■ Failed to save default model: io error'], refused.checkpoint),
	).toBe(false);
	expect(() => modelJourney('cursor', [], 'default', [], 8_000)).toThrow();
});
