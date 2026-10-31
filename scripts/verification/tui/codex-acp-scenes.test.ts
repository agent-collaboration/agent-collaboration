import { expect, test } from 'vitest';

import {
	CODEX_0_165_TEXT,
	compactJourney,
	composedSkillText,
	encodeCodexAction,
	codexEffortLabel,
	codexFixtureTitleSchema,
	codexSkillDisplayName,
	gridShowsSelectedMenuRow,
	gridShowsCheckpoint,
	interruptJourney,
	modelJourney,
	pickerOnlyJourney,
	renameJourney,
	reopenJourney,
	requireCheckpointSequence,
	skillJourney,
} from './codex-acp-scenes.ts';
import type { CodexCheckpoint, ModelJourneyInput, SkillJourneyInput } from './codex-acp-scenes.ts';

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
		[modelJourney(MODEL_INPUT), MODEL_ROW, '› /model'],
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

test('/model waits for the refreshed catalogue and each selected row before its next key', () => {
	const journey = modelJourney(MODEL_INPUT);
	expect(journey.name).toBe('codex-acp-j2-claude-model-default');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'model-command-ready',
		'model-catalogue-ready',
		'model-row-selected',
		'reasoning-popup-open',
		'effort-selected',
		'model-applied',
	]);
	// Only the declared keys: Enter opens, the menu keys move, Enter applies.
	expect(journey.steps.map((step) => step.actions)).toEqual([
		[{ kind: 'text', text: '/model' }],
		[{ kind: 'key', key: 'Enter' }],
		[{ kind: 'key', key: 'Down' }],
		[{ kind: 'key', key: 'Enter' }],
		[{ kind: 'key', key: 'Down' }],
		[{ kind: 'key', key: 'Enter' }],
	]);
	const [, catalogue, row, reasoning, effort, applied] = journey.steps.map(
		(step) => step.checkpoint,
	);
	if (!catalogue || !row || !reasoning || !effort || !applied)
		throw new Error('model scene missing');
	const refreshed = [
		'  Select Model and Effort',
		'› 1. Sonnet 4.6 (current)  Balanced',
		'  2. Haiku 5.5  Fast',
	];
	expect(gridShowsCheckpoint(refreshed, catalogue)).toBe(true);
	for (const screen of [
		// The cached bootstrap view before FetchModels returns.
		['  Select Model and Effort', '› 1. Use provider settings (current)'],
		// The refreshed rows with the stale bootstrap row still drawn.
		[...refreshed, '  3. Use provider settings'],
		// The command still in the composer.
		['› /model'],
		// Another catalogue: the declared page rows are missing.
		['  Select Model and Effort', '› 1. Other Model (current)', '  2. Another Model'],
	])
		expect(gridShowsCheckpoint(screen, catalogue), screen.join('|')).toBe(false);
	// With no stale rows declared (the bound list keeps the delegation), only the page proves refresh.
	const keptDelegation = modelJourney({ ...MODEL_INPUT, staleRows: [] }).steps[1]?.checkpoint;
	if (keptDelegation === undefined) throw new Error('catalogue scene missing');
	expect(
		gridShowsCheckpoint(
			['  Select Model and Effort', '› 1. Use provider settings (current)'],
			keptDelegation,
		),
	).toBe(false);
	expect(
		gridShowsCheckpoint(
			['  Select Model and Effort', '  1. Sonnet 4.6 (current)', '› 2. Haiku 5.5  Fast'],
			row,
		),
	).toBe(true);
	for (const selected of [
		'› 1. Sonnet 4.6 (current)',
		'  2. Haiku 5.5  Fast',
		'› 2. Haiku 5.5 (current)',
		'› 3. Haiku 5.5',
		'› 2. Haiku 5.5x',
	])
		expect(gridShowsCheckpoint(['  Select Model and Effort', selected], row), selected).toBe(false);
	expect(
		gridShowsCheckpoint(
			['  Select Reasoning Level for Haiku 5.5', '› 1. Low (default)'],
			reasoning,
		),
	).toBe(true);
	expect(gridShowsCheckpoint(['  Select Reasoning Level for Sonnet 4.6'], reasoning)).toBe(false);
	expect(
		gridShowsCheckpoint(
			['  Select Reasoning Level for Haiku 5.5', '  1. Low (default)', '› 2. High'],
			effort,
		),
	).toBe(true);
	// Codex labels the native value; the raw value, another marker or another row is not this row.
	for (const selected of [
		'› 2. high',
		'› 2. High (default)',
		'› 2. High (current)',
		'› 1. Low (default)',
	])
		expect(
			gridShowsCheckpoint(['  Select Reasoning Level for Haiku 5.5', selected], effort),
			selected,
		).toBe(false);
	expect(
		gridShowsCheckpoint(
			['• Model changed to claude-haiku-5-5 high for this conversation'],
			applied,
		),
	).toBe(true);
	for (const screen of [
		[
			'• Model changed to claude-haiku-5-5 high for this conversation',
			'  Select Reasoning Level for Haiku 5.5',
		],
		['• Model changed to claude-haiku-5-5 high for this conversation', '  Select Model and Effort'],
		['› /model'],
	])
		expect(gridShowsCheckpoint(screen, applied), screen.join('|')).toBe(false);
});

test('/model folds a selection already drawn into the frame that shows it, and refuses advanced efforts', () => {
	// The target is already selected when the refreshed catalogue appears: no navigation step.
	const direct = modelJourney({
		...MODEL_INPUT,
		modelNavigation: [],
		target: { displayName: 'Sonnet 4.6', number: 1, isCurrent: true },
		effort: null,
		apply: 'session',
	});
	expect(direct.steps.map((step) => step.checkpoint.scene)).toEqual([
		'model-command-ready',
		'model-catalogue-ready',
		'model-applied',
	]);
	expect(direct.steps[1]?.checkpoint.selected_menu_row).toEqual({
		number: 1,
		label: 'Sonnet 4.6',
		default: false,
		current: true,
	});
	expect(direct.steps[2]?.actions).toEqual([{ kind: 'text', text: 's' }]);
	const effortFirst = modelJourney({
		...MODEL_INPUT,
		effort: { ...MODEL_EFFORT, navigation: [], number: 1, value: 'low', isDefault: true },
	});
	expect(effortFirst.steps.map((step) => step.checkpoint.scene)).not.toContain('effort-selected');
	expect(effortFirst.steps[3]?.checkpoint.selected_menu_row).toEqual({
		number: 1,
		label: 'Low',
		default: true,
		current: false,
	});
	for (const value of ['max', 'ultra'])
		expect(() => modelJourney({ ...MODEL_INPUT, effort: { ...MODEL_EFFORT, value } })).toThrow(
			'More reasoning',
		);
	expect(() => modelJourney({ ...MODEL_INPUT, cataloguePage: [] })).toThrow('catalogue page');
	expect(codexEffortLabel('xhigh')).toBe('Extra high');
	expect(codexEffortLabel('provider')).toBe('provider');
});

// A placeholder refusal for the oracle; a real plan declares the endpoint's text from its source.
const MODEL_EFFORT = {
	navigation: [{ kind: 'key', key: 'Down' }],
	value: 'high',
	number: 2,
	isDefault: false,
	isCurrent: false,
} as const;
const MODEL_INPUT: ModelJourneyInput = {
	provider: 'claude',
	cataloguePage: ['Sonnet 4.6', 'Haiku 5.5'],
	staleRows: ['Use provider settings'],
	modelNavigation: [{ kind: 'key', key: 'Down' }],
	target: { displayName: 'Haiku 5.5', number: 2, isCurrent: false },
	effort: MODEL_EFFORT,
	apply: 'default',
	expectedOutcome: ['Model changed to claude-haiku-5-5 high'],
	outcomeDeadlineMs: 8_000,
};

const RENAME_REFUSAL = 'Renaming this thread is not supported here';

test('rename waits for the drawn /rename line, then needs the refusal, an empty composer and no exit', () => {
	const journey = renameJourney('cursor', 'Fixture Title', [RENAME_REFUSAL]);
	expect(journey.name).toBe('codex-acp-j2-cursor-rename');
	const [ready, refused] = journey.steps;
	if (ready === undefined || refused === undefined) throw new Error('rename scene missing');
	expect(ready.actions).toEqual([{ kind: 'text', text: '/rename Fixture Title' }]);
	expect(refused.actions).toEqual([{ kind: 'key', key: 'Enter' }]);
	// The popup is closed once a title follows the command, so the drawn composer line is the proof.
	expect(gridShowsCheckpoint(['› /rename Fixture Title'], ready.checkpoint)).toBe(true);
	expect(gridShowsCheckpoint(['› /rename Fixture'], ready.checkpoint)).toBe(false);
	// Typed during the startup draft, before the resume lands: Enter must wait.
	expect(
		gridShowsCheckpoint(['  Resuming session…', '› /rename Fixture Title'], ready.checkpoint),
	).toBe(false);
	expect(gridShowsCheckpoint(['', ''], ready.checkpoint)).toBe(false);
	const emptyComposer = `› Ask Codex to do anything${' '.repeat(94)}`;
	expect(gridShowsCheckpoint([`■ ${RENAME_REFUSAL}`, '', emptyComposer], refused.checkpoint)).toBe(
		true,
	);
	for (const screen of [
		// The CLI exited or the screen is blank.
		[],
		['', ''],
		// The command is still in the composer.
		[`■ ${RENAME_REFUSAL}`, '› /rename Fixture Title'],
		// A TUI-local error without the endpoint's refusal.
		['■ Renaming is blocked while this thread is read-only', emptyComposer],
		// The declared text outside an error cell.
		['■ an earlier error cell', `• ${RENAME_REFUSAL}`, emptyComposer],
		[`• ${RENAME_REFUSAL}`, emptyComposer],
	])
		expect(gridShowsCheckpoint(screen, refused.checkpoint), screen.join('|')).toBe(false);
	expect(() => renameJourney('cursor', 'Fixture Title', [])).toThrow();
});

test('a fixture title cannot carry keys, controls or padding', () => {
	expect(codexFixtureTitleSchema.parse('Fixture Title-2_a')).toBe('Fixture Title-2_a');
	for (const title of [
		'',
		' Fixture',
		'Fixture ',
		'a\rb',
		'a\u001b[Ab',
		'/quit',
		'a;b',
		'x'.repeat(41),
	])
		expect(() => renameJourney('claude', title, [RENAME_REFUSAL]), JSON.stringify(title)).toThrow();
});

test('the default apply can declare the honest default-save refusal as its outcome', () => {
	const applied = modelJourney({
		...MODEL_INPUT,
		expectedOutcome: ['Failed to save default model', ROUTER_SAVE_REFUSAL],
	}).steps.at(-1)?.checkpoint;
	if (applied === undefined) throw new Error('model refusal scene missing');
	expect(
		gridShowsCheckpoint([`■ Failed to save default model: ${ROUTER_SAVE_REFUSAL}`], applied),
	).toBe(true);
	expect(gridShowsCheckpoint(['■ Failed to save default model: io error'], applied)).toBe(false);
});

test('a selected menu row ends at its declared name, before the description column or the line end', () => {
	const haiku = { number: 2, label: 'Haiku', default: false, current: false };
	// The description column starts two cells after the widest name (selection_popup_common.rs:137-186).
	for (const line of ['› 2. Haiku  Fast model', '  › 2. Haiku', '› 2. Haiku   '])
		expect(gridShowsSelectedMenuRow([line], haiku), line).toBe(true);
	// One space after the declared name means the name continues, or an undeclared marker follows.
	for (const line of [
		'› 2. Haiku Other',
		'› 2. Haiku 5.5  Fast',
		'› 2. Haiku (current)',
		'› 2. Haiku (default)  Fast',
	])
		expect(gridShowsSelectedMenuRow([line], haiku), line).toBe(false);
	const lowDefault = { number: 1, label: 'Low', default: true, current: false };
	expect(gridShowsSelectedMenuRow(['› 1. Low (default)  Fastest responses'], lowDefault)).toBe(
		true,
	);
	for (const line of [
		'› 1. Low (default) (current)',
		'› 1. Low Extra (default)',
		'› 1. Low (defaults)',
	])
		expect(gridShowsSelectedMenuRow([line], lowDefault), line).toBe(false);
	// Effort labels under the same rule: Codex's label for the value, nothing appended.
	const high = { number: 2, label: codexEffortLabel('high'), default: false, current: false };
	for (const line of ['› 2. High Extra', '› 2. High plus'])
		expect(gridShowsSelectedMenuRow([line], high), line).toBe(false);
});
