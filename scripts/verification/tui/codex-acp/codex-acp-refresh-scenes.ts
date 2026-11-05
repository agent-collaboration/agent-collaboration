/**
 * J3 active-turn skills refresh in the unchanged Codex 0.165 TUI. One ordinary prompt runs; while
 * it is still working, `/skills` opens its menu (available during a task, slash_command.rs:249-312),
 * "Enable/Disable Skills" opens the manage view, and Esc closes it without toggling. Closing sends
 * ManageSkillsClosed, which lists skills again with forceReload on the same app-server connection
 * (skills_toggle_view.rs:183-189; chatwidget/skills.rs:103-104). The turn then ends with its own
 * reply, uninterrupted, and Codex exits through `/quit`. The screen shows neither the reload nor
 * its result, so nothing here claims a disk reload or a new provider skill; the same-session
 * observer decides the reply. Expected text comes from Codex source at 360319d8.
 */
import { requireOrdinaryReply } from './codex-acp-prompt-scenes.ts';
import { CODEX_0_165_TEXT, UI_CHECKPOINT_MS, codexJourneySchema } from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

export interface RefreshJourneyInput {
	readonly provider: 'claude' | 'cursor';
	readonly prompt: string;
	// One line only the provider's reply produces; it is never typed or drawn by Codex itself.
	readonly completion: string;
	readonly replyDeadlineMs: number;
}

/**
 * The prompt is composed, submitted and seen working before any other key. While a bottom-pane
 * view is open only the view is drawn, without the status row (bottom_pane/mod.rs:2418-2453), so
 * the menu and manage-view frames cannot show "Working". The turn's continuity is decided by the
 * frames around them instead: every frame forbids the completion and the interruption, the frame
 * after Esc shows "Working" again with an empty composer, and only the declared keys are sent.
 * Space and Enter toggle a skill in the manage view, so the only key there is Esc, which closes the
 * view and never interrupts the turn (bottom_pane/mod.rs:925-953). A toggle would draw its count
 * on close (chatwidget/skills.rs:134).
 */
export function refreshJourney(input: RefreshJourneyInput): CodexJourney {
	requireOrdinaryReply(input.prompt, input.completion, ['/skills']);
	const ended = [input.completion, CODEX_0_165_TEXT.interrupted];
	return codexJourneySchema.parse({
		name: `codex-acp-j3-${input.provider}-refresh`,
		steps: [
			{
				actions: [{ kind: 'text', text: input.prompt }],
				checkpoint: {
					scene: 'refresh-prompt-composed',
					require: [CODEX_0_165_TEXT.composerPrompt + input.prompt],
					forbid: [CODEX_0_165_TEXT.working, CODEX_0_165_TEXT.resumingSession, ...ended],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'refresh-turn-running',
					require: [CODEX_0_165_TEXT.working],
					forbid: ended,
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'text', text: '/skills' }],
				checkpoint: {
					scene: 'refresh-skills-command-ready',
					require: [CODEX_0_165_TEXT.working, CODEX_0_165_TEXT.skillsCommandRow],
					forbid: ended,
					deadline_ms: UI_CHECKPOINT_MS,
					selected_slash_row: { command: 'skills', description: CODEX_0_165_TEXT.skillsCommandRow },
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'refresh-skills-menu-open',
					require: [
						CODEX_0_165_TEXT.skillsMenuSubtitle,
						CODEX_0_165_TEXT.skillsMenuList,
						CODEX_0_165_TEXT.skillsMenuManage,
					],
					forbid: [CODEX_0_165_TEXT.skillsCommandRow, ...ended],
					deadline_ms: UI_CHECKPOINT_MS,
					selected_menu_row: {
						number: 1,
						label: CODEX_0_165_TEXT.skillsMenuList,
						default: false,
						current: false,
					},
				},
			},
			{
				actions: [{ kind: 'key', key: 'Down' }],
				checkpoint: {
					scene: 'refresh-manage-selected',
					require: [CODEX_0_165_TEXT.skillsMenuSubtitle],
					forbid: ended,
					deadline_ms: UI_CHECKPOINT_MS,
					selected_menu_row: {
						number: 2,
						label: CODEX_0_165_TEXT.skillsMenuManage,
						default: false,
						current: false,
					},
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'refresh-manage-open',
					require: [
						CODEX_0_165_TEXT.skillsMenuManage,
						CODEX_0_165_TEXT.manageSkillsSubtitle,
						CODEX_0_165_TEXT.manageSkillsHint,
					],
					forbid: [CODEX_0_165_TEXT.skillsMenuSubtitle, CODEX_0_165_TEXT.noSkills, ...ended],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Escape' }],
				checkpoint: {
					scene: 'refresh-manage-closed',
					require: [CODEX_0_165_TEXT.working],
					forbid: [
						CODEX_0_165_TEXT.manageSkillsSubtitle,
						CODEX_0_165_TEXT.manageSkillsHint,
						CODEX_0_165_TEXT.skillsMenuSubtitle,
						CODEX_0_165_TEXT.skillsToggled,
						...ended,
					],
					deadline_ms: UI_CHECKPOINT_MS,
					empty_composer: true,
				},
			},
			{
				// No key: the running turn ends by itself with its own reply.
				actions: [],
				checkpoint: {
					scene: 'refresh-reply-completed',
					require: [input.completion],
					forbid: [
						CODEX_0_165_TEXT.working,
						CODEX_0_165_TEXT.interrupted,
						CODEX_0_165_TEXT.skillsToggled,
					],
					deadline_ms: input.replyDeadlineMs,
					empty_composer: true,
				},
			},
		],
	});
}
