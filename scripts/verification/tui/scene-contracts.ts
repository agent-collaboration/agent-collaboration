import type { TerminalViewport } from './evidence-schema.ts';

export const viewports: readonly TerminalViewport[] = [
	{ columns: 120, rows: 36 },
	{ columns: 76, rows: 30 },
	{ columns: 48, rows: 24 },
	{ columns: 48, rows: 12 },
	{ columns: 40, rows: 24 },
	{ columns: 32, rows: 24 },
];
export const componentFixtureTest =
	'quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture';
export const captureTest = 'compiled_compact_current_storyboard_exports_both_real_tuis';
export const quotaCaptureTest = 'compiled_compact_current_quota_storyboard_exports_real_tui';
export const sessionsCaptureTest = 'compiled_compact_current_sessions_storyboard_exports_real_tui';
export const normalComponentCaptureTest = 'compiled_normal_quota_component_dialogs_export_real_tui';
export const hardComponentCaptureTest = 'compiled_hard_case_quota_component_exports_real_tui';
export const supportDirectory =
	'crates/codex-router-quota-reset-test-harness/tests/e2e/quota_storyboard/quota_storyboard_capture_test';
export const sessionStates = [
	'all',
	'codex-details',
	'fork',
	'fork-choice',
	'new',
	'new-machine',
	'new-no-match',
	'new-returned',
	'machine-sources',
	'client-no-match',
	'chooser',
	'help',
	'claude',
	'cancelled',
	'details',
	'folder-choice',
	'model-provider-choice',
	'source-choice',
	'runtime-choice',
	'sort-choice',
];
export interface DetailsPose {
	readonly name: string;
	readonly down: number;
}
export function sessionDetailsPoses(grid: TerminalViewport): readonly DetailsPose[] {
	if (grid.columns === 48 && grid.rows === 12)
		return [
			{ name: 'details-identity', down: 0 },
			{ name: 'details-state', down: 3 },
			{ name: 'details-path', down: 9 },
			{ name: 'details-title', down: 12 },
		];
	const offsets =
		grid.columns === 120
			? [0, 0, 0]
			: grid.columns === 76
				? [0, 2, 5]
				: grid.columns === 48
					? [0, 1, 4]
					: [0, 4, 8];
	return ['details', 'details-path', 'details-title'].map((name, index) => ({
		name,
		down: offsets[index] ?? 0,
	}));
}
export const sessionDetailsFields = [
	'details context claude gamma [000.0011]',
	'details title claude gamma session · project-a',
	'details path /repo/project-a',
	'details session id 00000000-0000-4000-8000-000000000011',
	'details endpoint claude gamma',
	'details state idle',
];
export function sessionCheckpointStates(grid: TerminalViewport): string[] {
	return sessionStates.flatMap((state) =>
		state === 'details' ? sessionDetailsPoses(grid).map((pose) => pose.name) : [state],
	);
}
export function sessionDetailsRequiredFields(checkpoint: string): readonly string[] {
	const state = checkpoint.replace(/^compact-sessions-/u, '');
	const indices =
		state === 'details-title'
			? [0, 1]
			: state === 'details-path'
				? [0, 2]
				: state === 'details-identity'
					? [0, 3]
					: state === 'details-state'
						? [0, 4, 5]
						: state === 'details'
							? [0, 3, 4, 5]
							: [];
	return indices.map((index) => {
		const field = sessionDetailsFields[index];
		if (field === undefined) throw new Error('invalid Details field index');
		return field;
	});
}
export function quotaCheckpointStates(rows: number): string[] {
	return [
		'all',
		'pool-details',
		...(rows >= 36 ? ['pool-details-complete'] : []),
		'pool-details-middle',
		'pool-details-bottom',
		'pool-closed',
		'filters-block-pool',
		'provider-no-match',
		'chooser',
		'help',
		'disabled-choice',
		'hidden-choice',
		'claude',
		'cancelled',
		'details',
	];
}
export const quotaNegativePairs: readonly (readonly [string, string])[] = [
	['hidden-choice', 'disabled-choice'],
	['pool-details', 'all'],
	['pool-closed', 'pool-details'],
];
export const normalComponentStates = [
	'browse',
	'browse-credit-window',
	'browse-pace-heading',
	'browse-pace-value',
	'browse-page-returned',
	'next-account',
	'restored-account',
	'details-top',
	'details-bottom',
	'help-top',
	'help-bottom',
	'floor-original',
	'floor-draft',
	'floor-cancelled',
	'floor-saved',
	'options-identity',
	'reset-inventory',
	'reset-last-page',
	'credits-identity',
	'credits-balance',
	'policy-original',
	'policy-choice',
	'policy-no-match',
	'policy-choice-returned',
	'policy-draft',
	'policy-cancelled',
	'policy-saved',
	'refresh-result',
	'credits-refreshed-balance',
	'credits-refreshed-freshness',
	'credits-refreshed-observation',
	'credits-policy-boundary',
	'reset-review',
	'confirm-no',
	'confirm-no-returned',
	'confirm-yes',
	'confirm-help-top',
	'confirm-help-identity',
	'confirm-help-returned',
	'reset-result',
	'reset-result-consumption',
	'reset-result-bottom',
	'returned',
];
export const hardCaseStates = [
	'hard-browse',
	'hard-beta-windows',
	'hard-beta-extra',
	'hard-pool-details',
	'hard-pool-details-complete',
	'hard-pool-details-middle',
	'hard-pool-details-bottom',
	'hard-pool-closed',
	'hard-details',
];
export const hardCasePartialStates = hardCaseStates.filter(
	(state) => state !== 'hard-pool-details-complete',
);
export const hardCaseShortStates = [
	'hard-browse-top',
	'hard-alpha-name-continued',
	'hard-alpha-week',
	'hard-alpha-short',
	'hard-beta-selected',
	'hard-beta-name-continued',
	'hard-beta-week',
	'hard-beta-short',
	'hard-beta-extra',
	'hard-pool-details',
	'hard-pool-details-middle',
	'hard-pool-details-bottom',
	'hard-pool-closed-top',
	'hard-details',
];
export const hardCaseNegatives: readonly (readonly [string, string])[] = [
	['component-quota-hard-beta-windows', 'blank-startup'],
	['component-quota-hard-beta-windows', 'component-quota-hard-browse'],
	['component-quota-hard-beta-extra', 'component-quota-hard-browse'],
	['component-quota-hard-pool-details', 'component-quota-hard-beta-windows'],
	['component-quota-hard-pool-closed', 'component-quota-hard-pool-details'],
	['component-quota-hard-pool-details-bottom', 'component-quota-hard-details'],
];
export const hardCaseShortNegatives: readonly (readonly [string, string])[] = [
	['component-quota-hard-beta-selected', 'blank-startup'],
	['component-quota-hard-beta-selected', 'component-quota-hard-browse-top'],
	['component-quota-hard-beta-week', 'component-quota-hard-alpha-week'],
	['component-quota-hard-beta-short', 'component-quota-hard-alpha-short'],
	['component-quota-hard-pool-details', 'component-quota-hard-beta-selected'],
	['component-quota-hard-pool-closed-top', 'component-quota-hard-pool-details'],
	['component-quota-hard-pool-details-bottom', 'component-quota-hard-details'],
	['component-quota-hard-alpha-name-continued', 'component-quota-hard-beta-name-continued'],
	['component-quota-hard-beta-name-continued', 'component-quota-hard-alpha-name-continued'],
];
export const normalComponentNegatives: readonly (readonly [string, string])[] = [
	['policy-saved', 'blank-startup'],
	['policy-saved', 'component-quota-policy-cancelled'],
	['floor-saved', 'component-quota-floor-cancelled'],
	['floor-saved', 'component-quota-floor-draft'],
	['credits-refreshed-balance', 'component-quota-credits-balance'],
	['returned', 'component-quota-reset-result'],
	['help-bottom', 'component-quota-help-top'],
	['details-bottom', 'component-quota-details-top'],
	['reset-result-bottom', 'component-quota-confirm-yes'],
	['reset-result-consumption', 'component-quota-confirm-yes'],
	['browse-credit-window', 'component-quota-next-account'],
	['browse-pace-heading', 'component-quota-next-account'],
	['browse-pace-value', 'component-quota-reset-result'],
].map(([scene, wrong]) => {
	if (scene === undefined || wrong === undefined) throw new Error('incomplete negative pair');
	return [`component-quota-${scene}`, wrong];
});
export function componentDialog(state: string): string {
	if (state.startsWith('hard-pool-details')) return 'Pool details';
	if (state === 'hard-details') return 'Account details';
	if (
		state.startsWith('hard-') ||
		state.startsWith('browse') ||
		['next-account', 'restored-account', 'returned'].includes(state)
	)
		return 'browse';
	if (state.startsWith('details-')) return 'Account details';
	if (state.startsWith('help-') || ['confirm-help-top', 'confirm-help-identity'].includes(state))
		return 'Quota Help';
	if (state.startsWith('floor-')) return 'Weekly floor';
	if (state.startsWith('policy-')) return 'Credit usage';
	if (state.startsWith('reset-result')) return 'Reset result';
	if (['confirm-no', 'confirm-yes', 'confirm-help-returned'].includes(state))
		return 'Confirm reset';
	if (state.startsWith('credits-') || state === 'refresh-result') return 'Options · Credits';
	return 'Options · Resets';
}
export function checkpointDialog(state: string): string {
	if (state.startsWith('new')) return 'new';
	if (state.startsWith('fork')) return 'fork';
	if (state === 'machine-sources') return 'machines';
	if (state.startsWith('pool-details')) return 'pool-details';
	if (
		['chooser', 'filters-block-pool'].includes(state) ||
		state.endsWith('choice') ||
		state.endsWith('no-match')
	)
		return 'filters';
	if (state.startsWith('details') || state.endsWith('-details')) return 'details';
	return state === 'help' ? 'contextual-help' : 'browse';
}
export const proofLimits =
	'Synthetic records and substituted external effects; no live discovery, provider launch, personal balances, account reset/save or Ghostty observation. Cell assertions check scene markers; visual inspection and offered-product-test sensitivity remain required.';
export function coverageGaps(): string[] {
	return [
		'sessions/new',
		'sessions/machine-choice',
		'sessions/fork',
		'sessions/Details-Left-Right-Tab-focus-Enter-exact-effect-public-entry',
		'sessions/source-loading-and-failure',
		'sessions/choice-no-match-unavailable',
		'sessions/full-ID-paste-and-all-filter-fields',
		'agent-sessions/real-compiled-public-entry-Codex-ClaudeCode-Cursor',
		'agent-sessions/client-actions-hosted-Claude-interactive-Claude-hosted-Cursor',
		'agent-sessions/provider-runtime-proof-grant-and-adopted-action-contract',
		'R16/actual-callable-Human-Load-and-Session-requester-preservation',
		'R16/source-Stored-summary-saved-policy-directory-other-Approver',
		'R16/inventory-Stored-retention-explicit-partial-and-read-failure-classification',
		'R16/real-local-and-remote-managed-Cursor-create-load-resume',
		'R16/typed-origin-and-native-job-handle-versus-session-UUID',
		'R16/managed-one-shot-unknown-with-Router-operation-readback',
		'R16/native-lost-receipt-unknown-no-fake-Router-ID-or-retry',
		'R16/catalogue-real-bounded-native-job-reader-and-mixed-vendor-shapes',
		'R16/catalogue-new-Host-service-API-native-Stored-Loaded-Active-and-pagination',
		'R16/catalogue-independent-peer-job-coverage-through-rendered-notice',
		'R16/catalogue-captured-native-UUID-directory-source-and-terminal-job-resume',
		'R16/catalogue-before-first-prompt-public-TUI-local-and-Sunbook',
		'R15/public-agent-sessions-causal-background-process-proof',
		'R15/actual-native-print-background-guard-before-session-dispatch',
		'R15/native-Claude-background-registration-and-owned-cleanup',
		'R15/TUI-launch-owner-reuse-awaits-admitted-U16-implementation-and-target',
		'quota/options-resets',
		'quota/options-credits',
		'quota/credit-policy',
		'quota/credit-refresh',
		'quota/weekly-floor',
		'quota/reset-inventory',
		'quota/reset-review',
		'quota/reset-confirmation',
		'quota/reset-result',
		'quota/paging',
		'48x12 usable header/actions and scrolled body (static capture wired, unobserved)',
		'40x24 and 32x24 physical cell bounds and original visibility (wired, unobserved)',
		'actual iPhone terminal operation (separate device target, unobserved)',
		'C7 named header and current Help census associations',
		'C7 active field/query/no-match and unavailable Select',
		'C7 captured decision identity/risk and single No/Yes choice group',
		'C7 short-body action adjacency and long-body final content',
		'resize continuity',
		'long-name-CJK-and-status-variants',
		'offered-product-test sensitivity',
		'visual inspection',
		'U14 Lead document checks',
	];
}
