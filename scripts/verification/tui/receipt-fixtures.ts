/** Protocol fixtures exercise collectors, never substitute fabricated bytes for UI proof. */
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import type { CaptureSelection } from './capture-selection.ts';
import { capturePlanSchema, componentPlanSchema } from './evidence-schema.ts';
import type { CapturePlan, ComponentPlan, TerminalViewport } from './evidence-schema.ts';
import { captureArtifact } from './source-binding.ts';

export function temporaryRoot(): string {
	return realpathSync(mkdtempSync(join(tmpdir(), 'router-tui-contract-')));
}
export function saveJson(path: string, value: unknown): void {
	writeFileSync(path, JSON.stringify(value));
}
export function protocolPlan(root: string): CapturePlan {
	const binary = join(root, 'protocol-only-target');
	writeFileSync(binary, 'protocol only');
	const binding = captureArtifact(binary);
	const support = join(
		root,
		'crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test',
	);
	mkdirSync(support, { recursive: true });
	for (const name of [
		'provider_filter_capture.rs',
		'provider_filter_tape.rs',
		'verify_screen_image.swift',
		'verify_screen_checkpoint.swift',
		'quota_component_capture.rs',
		'quota_component_tape.rs',
	])
		writeFileSync(join(support, name), 'protocol only');
	const child = join(root, 'scripts/verification/tui/fixture-child.ts');
	mkdirSync(join(root, 'scripts/verification/tui'), { recursive: true });
	writeFileSync(child, 'protocol only');
	return capturePlanSchema.parse({
		repository: root,
		output: join(root, 'output'),
		temporary_directory: root,
		source: { head: 'a'.repeat(40), files: { 'protocol-only-target': binding.sha256 } },
		node: captureArtifact(process.execPath),
		capture_driver: binding,
		harness_binary: binding,
		vhs: binding,
		ttyd: binding,
		image_verifier: binding,
		dynamic_library_path: root,
		build_declaration: 'Protocol bytes only, not product or image proof',
	});
}
// Independent literal scene expectations, intentionally not imported from production contracts.
export const literalGrids: readonly TerminalViewport[] = [
	{ columns: 120, rows: 36 },
	{ columns: 76, rows: 30 },
	{ columns: 48, rows: 24 },
	{ columns: 48, rows: 12 },
	{ columns: 40, rows: 24 },
	{ columns: 32, rows: 24 },
];
export const literalSessionFields = [
	'details context claude gamma [000.0011]',
	'details title claude gamma session · project-a',
	'details path /repo/project-a',
	'details session id 00000000-0000-4000-8000-000000000011',
	'details endpoint claude gamma',
	'details state idle',
];
export const literalNormalStates =
	'browse browse-credit-window browse-pace-heading browse-pace-value browse-page-returned next-account restored-account details-top details-bottom help-top help-bottom floor-original floor-draft floor-cancelled floor-saved options-identity reset-inventory reset-last-page credits-identity credits-balance policy-original policy-choice policy-no-match policy-choice-returned policy-draft policy-cancelled policy-saved refresh-result credits-refreshed-balance credits-refreshed-freshness credits-refreshed-observation credits-policy-boundary reset-review confirm-no confirm-no-returned confirm-yes confirm-help-top confirm-help-identity confirm-help-returned reset-result reset-result-bottom returned'.split(
		' ',
	);
export const literalHardStates =
	'hard-browse hard-beta-windows hard-beta-extra hard-pool-details hard-pool-details-complete hard-pool-details-middle hard-pool-details-bottom hard-pool-closed hard-details'.split(
		' ',
	);
export const literalNormalNegatives: readonly (readonly [string, string])[] = [
	['policy-saved', 'blank-startup'],
	['policy-saved', 'component-quota-policy-cancelled'],
	['floor-saved', 'component-quota-floor-cancelled'],
	['floor-saved', 'component-quota-floor-draft'],
	['credits-refreshed-balance', 'component-quota-credits-balance'],
	['returned', 'component-quota-reset-result'],
	['help-bottom', 'component-quota-help-top'],
	['details-bottom', 'component-quota-details-top'],
	['reset-result-bottom', 'component-quota-confirm-yes'],
	['browse-credit-window', 'component-quota-next-account'],
	['browse-pace-heading', 'component-quota-next-account'],
	['browse-pace-value', 'component-quota-reset-result'],
];
export function negativeRecord(checkpoint: string): unknown {
	return {
		checkpoint,
		recognizedLines: [],
		recognizedFieldPairs: [],
		choiceFocusMeasurements: [],
		missingMarkers: ['required marker'],
		unexpectedMarkers: [],
	};
}
export function observationRecord(checkpoint: string, pairs: readonly string[] = []): unknown {
	return {
		checkpoint,
		recognizedLines:
			checkpoint === 'compact-quota-hidden-choice'
				? ['› Hidden']
				: checkpoint === 'compact-quota-disabled-choice'
					? ['› Shown']
					: ['protocol only'],
		recognizedFieldPairs: pairs,
		choiceFocusMeasurements: [],
		missingMarkers: [],
		unexpectedMarkers: [],
	};
}
function protocolImages(
	folder: string,
	scenes: readonly string[],
): { checkpoint: string; png_sha256: string }[] {
	return scenes.map((checkpoint) => {
		const path = join(folder, `${checkpoint}.png`);
		writeFileSync(
			path,
			Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), Buffer.alloc(1100, 42)]),
		);
		return { checkpoint, png_sha256: captureArtifact(path).sha256 };
	});
}
export function prepareStaticReceipts(root: string, selection: CaptureSelection): CapturePlan {
	const plan = protocolPlan(root);
	const surfaces =
		selection === 'static-quota'
			? ['quota']
			: selection === 'static-sessions'
				? ['sessions']
				: ['sessions', 'quota'];
	for (const surface of surfaces)
		for (const grid of literalGrids) {
			const folder = join(plan.output, `${surface}-compact-${grid.columns}x${grid.rows}`);
			mkdirSync(folder, { recursive: true });
			writeFileSync(join(folder, 'quota.tape'), 'protocol only');
			const poses =
				grid.columns === 48 && grid.rows === 12
					? ['details-identity', 'details-state', 'details-path', 'details-title']
					: ['details', 'details-path', 'details-title'];
			const states =
				surface === 'sessions'
					? 'all codex-details fork fork-choice new new-machine new-no-match new-returned machine-sources client-no-match chooser help claude cancelled DETAILS folder-choice model-provider-choice source-choice runtime-choice sort-choice'
							.split(' ')
							.flatMap((state) => (state === 'DETAILS' ? poses : [state]))
					: [
							'all',
							'pool-details',
							...(grid.rows === 36 ? ['pool-details-complete'] : []),
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
			const images = protocolImages(
				folder,
				states.map((state) => `compact-${surface}-${state}`),
			);
			for (const image of images) {
				const scene = image.checkpoint;
				saveJson(
					join(folder, `${scene}.image-observation.json`),
					observationRecord(
						scene,
						scene === 'compact-quota-chooser'
							? ['account provider openai', 'disabled accounts shown']
							: scene === 'compact-sessions-chooser'
								? ['client codex']
								: scene.startsWith('compact-sessions-details')
									? literalSessionFields
									: [],
					),
				);
			}
			const negatives: [string, string][] = [
				[`compact-${surface}-claude`, 'blank-startup'],
				[`compact-${surface}-claude`, `compact-${surface}-all`],
				[`compact-${surface}-cancelled`, `compact-${surface}-chooser`],
			];
			if (surface === 'quota')
				negatives.push(
					['compact-quota-hidden-choice', 'compact-quota-disabled-choice'],
					['compact-quota-pool-details', 'compact-quota-all'],
					['compact-quota-pool-closed', 'compact-quota-pool-details'],
				);
			if (surface === 'sessions') {
				for (const pose of poses)
					negatives.push([`compact-sessions-${pose}`, 'compact-sessions-codex-details']);
				if (grid.rows === 12)
					negatives.push(['compact-sessions-details-identity', 'compact-sessions-details-title']);
			}
			for (const [scene, wrong] of negatives)
				saveJson(
					join(folder, `reject-${scene}-${wrong}.image-observation.json`),
					negativeRecord(scene),
				);
			const digest = plan.harness_binary.sha256;
			saveJson(join(folder, 'filter-receipt.json'), {
				surface,
				...grid,
				node_sha256: plan.node.sha256,
				binary_sha256: digest,
				vhs_sha256: digest,
				ttyd_sha256: digest,
				verifier_sha256: digest,
				verifier_source_sha256: digest,
				verifier_checkpoint_source_sha256: digest,
				driver_source_sha256: digest,
				tape_driver_source_sha256: digest,
				fixed_child_sha256: digest,
				tape_sha256: digest,
				state_and_secrets_unchanged: true,
				provider_requests: 0,
				child_exit: 0,
				owned_processes_stopped: true,
				fixture: 'protocol only',
				images,
			});
		}
	return plan;
}
export function prepareComponentReceipts(
	root: string,
	grid: TerminalViewport = { columns: 120, rows: 36 },
	scenario: 'normal' | 'visual-hard-cases' = 'normal',
): ComponentPlan {
	const base = protocolPlan(root);
	const plan = componentPlanSchema.parse({
		...base,
		component_binary: base.harness_binary,
		grids: [grid],
		scenario,
	});
	const folder = join(
		plan.output,
		`quota-component-${scenario === 'normal' ? 'normal' : 'hard-cases'}-${grid.columns}x${grid.rows}`,
	);
	mkdirSync(folder, { recursive: true });
	writeFileSync(join(folder, 'quota.tape'), 'protocol only');
	const states = scenario === 'normal' ? literalNormalStates : literalHardStates;
	const images = protocolImages(
		folder,
		states.map((state) => `component-quota-${state}`),
	);
	for (const image of images) {
		const state = image.checkpoint.replace('component-quota-', '');
		const policy = state === 'policy-draft' || state === 'policy-saved' ? 'allow' : 'disallow';
		saveJson(
			join(folder, `${image.checkpoint}.image-observation.json`),
			observationRecord(
				image.checkpoint,
				[
					'policy-original',
					'policy-draft',
					'policy-cancelled',
					'policy-saved',
					'policy-choice-returned',
				].includes(state)
					? [`use credits ${policy}`]
					: state === 'floor-saved'
						? ['current 1%']
						: [],
			),
		);
	}
	const negatives: readonly (readonly [string, string])[] =
		scenario === 'normal'
			? literalNormalNegatives
			: [
					['hard-beta-windows', 'blank-startup'],
					['hard-beta-windows', 'component-quota-hard-browse'],
					['hard-beta-extra', 'component-quota-hard-browse'],
					['hard-pool-details', 'component-quota-hard-beta-windows'],
					['hard-pool-closed', 'component-quota-hard-pool-details'],
					['hard-pool-details-bottom', 'component-quota-hard-details'],
				];
	for (const [state, wrong] of negatives) {
		const scene = `component-quota-${state}`;
		saveJson(
			join(folder, `reject-${scene}-${wrong}.image-observation.json`),
			negativeRecord(scene),
		);
	}
	const digest = plan.component_binary.sha256;
	saveJson(join(folder, 'component-receipt.json'), {
		kind:
			scenario === 'normal'
				? 'compiled-quota-component-normal'
				: 'compiled-quota-component-hard-cases',
		...grid,
		node_sha256: plan.node.sha256,
		binary_sha256: digest,
		verifier_sha256: digest,
		checkpoint_source_sha256: digest,
		recognition_source_sha256: digest,
		driver_source_sha256: digest,
		tape_source_sha256: digest,
		fixed_child_sha256: digest,
		tape_sha256: digest,
		child_exit: 0,
		external_requests: 0,
		persisted_writes: 0,
		owned_children_stopped: true,
		images,
	});
	return plan;
}
export function originalBytes(path: string): Buffer {
	return readFileSync(path);
}
