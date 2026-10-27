import { isAbsolute } from 'node:path';

import { z } from 'zod';

export const sha256Schema = z.string().regex(/^[a-f0-9]{64}$/u);
export const pathSchema = z.string();
export const absolutePathSchema = pathSchema.refine(
	isAbsolute,
	'capture locations must be absolute',
);
export const sceneNameSchema = z
	.string()
	.min(1)
	.max(96)
	.regex(/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/u);
export const fixtureScenarioSchema = z.enum([
	'normal',
	'inspection-pending',
	'commit-pending',
	'unknown-result',
	'save-failed',
	'disabled-yes',
	'revalidation-pending',
	'refused-result',
	'refresh-pending',
	'save-pending',
	'visual-hard-cases',
]);
export type FixtureScenario = z.infer<typeof fixtureScenarioSchema>;
export const artifactBindingSchema = z
	.strictObject({ path: pathSchema, sha256: sha256Schema })
	.readonly();

export type ArtifactBinding = z.infer<typeof artifactBindingSchema>;
export const sourceBindingSchema = z
	.strictObject({
		head: z.string().regex(/^[a-f0-9]{40}$/u),
		files: z.record(z.string(), sha256Schema).refine((files) => Object.keys(files).length > 0),
	})
	.readonly();

export type SourceBinding = z.infer<typeof sourceBindingSchema>;
export const viewportSchema = z
	.strictObject({
		columns: z.number().int().positive(),
		rows: z.number().int().positive(),
	})
	.readonly();

export type TerminalViewport = z.infer<typeof viewportSchema>;
export const choiceFocusSchema = z
	.strictObject({
		label: z.enum(['shown', 'hidden']),
		labelYellowPixels: z.number().int().nonnegative(),
		glyphYellowPixels: z.number().int().nonnegative(),
		glyphColumns: z.number().int().nonnegative(),
		glyphRows: z.number().int().nonnegative(),
		labelHeight: z.number().int().positive(),
	})
	.readonly();

export type ChoiceFocusMeasurement = z.infer<typeof choiceFocusSchema>;
const observationFields = {
	checkpoint: z.string(),
	recognizedLines: z.array(z.string()),
	recognizedFieldPairs: z.array(z.string()),
	choiceFocusMeasurements: z.array(choiceFocusSchema),
	missingMarkers: z.array(z.string()),
	unexpectedMarkers: z.array(z.string()),
};
export const imageObservationSchema = z
	.strictObject({
		...observationFields,
		recognizedLines: z.array(z.string()).min(1),
		missingMarkers: z.array(z.string()).max(0),
		unexpectedMarkers: z.array(z.string()).max(0),
	})
	.readonly();

export type ImageObservation = z.infer<typeof imageObservationSchema>;
export const imageRejectionSchema = z
	.strictObject(observationFields)
	.refine(
		(value) => value.missingMarkers.length + value.unexpectedMarkers.length > 0,
		'negative control accepted the wrong scene',
	)
	.readonly();
export type ImageRejection = z.infer<typeof imageRejectionSchema>;

export function provesChoiceFocus(measurement: ChoiceFocusMeasurement): boolean {
	return (
		measurement.labelYellowPixels >= 10 &&
		measurement.glyphYellowPixels >= 4 &&
		measurement.glyphColumns >= 2 &&
		measurement.labelHeight >= 4 &&
		measurement.glyphRows >= Math.max(3, Math.floor(measurement.labelHeight / 3))
	);
}
export function requireSelectedChoice(
	observation: ImageObservation,
	expected: 'shown' | 'hidden',
): void {
	const text = observation.recognizedLines.join(' ').toLowerCase();
	const other = expected === 'hidden' ? 'shown' : 'hidden';
	if (!new RegExp(`[›❯>]\\s*${other}\\b`, 'u').test(text)) {
		if (new RegExp(`[›❯>]\\s*${expected}\\b`, 'u').test(text)) return;
		const focused = observation.choiceFocusMeasurements.filter(provesChoiceFocus);
		if (
			focused.length === 1 &&
			focused[0]?.label === expected &&
			observation.choiceFocusMeasurements.filter((item) => item.label === expected).length === 1
		)
			return;
	}
	throw new Error('selected choice lacks original glyph/color evidence');
}
function ownedLoopbackUrl(value: string, exported: boolean): boolean {
	try {
		const url = new URL(value);
		if (
			url.protocol !== 'http:' ||
			url.hostname !== '127.0.0.1' ||
			!url.port ||
			url.username ||
			url.password ||
			url.search ||
			url.hash
		)
			return false;
		return exported ? /^\/export\/[a-f0-9]{64}\/$/u.test(url.pathname) : url.pathname === '/';
	} catch {
		return false;
	}
}
export const cuaExportSchema = z
	.strictObject({
		base_url: z
			.string()
			.refine(
				(value) => ownedLoopbackUrl(value, true),
				'exact private loopback nonce URL required',
			),
		directory: pathSchema,
		scenes: z
			.array(sceneNameSchema)
			.min(1)
			.max(128)
			.refine(
				(values) => new Set(values).size === values.length,
				'duplicate declared original-artifact scene',
			),
		maximum_bytes: z.literal(8388608),
		mime_type: z.literal('image/jpeg'),
		source_sha256: sha256Schema,
	})
	.readonly();

export const cuaReadySchema = z
	.strictObject({
		kind: z.literal('listener-ready-not-ui'),
		surface: z.enum(['sessions', 'quota']),
		url: z
			.string()
			.refine((value) => ownedLoopbackUrl(value, false), 'exact loopback listener required'),
		ttyd_pid: z.number().int().positive(),
		binary_sha256: z.string(),
		node_sha256: sha256Schema,
		ttyd_sha256: z.string(),
		child_pid_file: pathSchema,
		viewport_file: pathSchema,
		stop_file: pathSchema,
		lifetime_seconds: z.literal(600),
		artifact_export: cuaExportSchema,
		quota_fixture_scenario: fixtureScenarioSchema.nullable().default(null),
	})
	.readonly();

export const cuaLifecycleSchema = z
	.strictObject({
		kind: z.literal('live-cua-fixture-lifecycle-only'),
		surface: z.enum(['sessions', 'quota']),
		binary_sha256: z.string(),
		node_sha256: sha256Schema,
		child_exit: z.literal(0),
		provider_requests: z.literal(0),
		state_and_secrets_unchanged: z.literal(true),
		owned_processes_stopped: z.literal(true),
		artifact_sink_stopped: z.literal(true),
		artifact_count: z.number().int().nonnegative(),
		last_viewport: z.string(),
		ui_observation: z.literal('separate-original-CUA-evidence-required'),
		quota_fixture_scenario: fixtureScenarioSchema.nullable().default(null),
	})
	.readonly();

export const cuaSavedArtifactSchema = z
	.strictObject({
		kind: z.literal('original-cua-jpeg-bytes-only'),
		scene: sceneNameSchema,
		filename: z.string(),
		bytes: z.number().int().positive().max(8388608),
		sha256: sha256Schema,
		encoding_round_trip: z.literal(true),
	})
	.readonly();

export const processIdentitySchema = z
	.strictObject({
		pid: z.number().int(),
		uid: z.number().int(),
		group: z.number().int(),
		birth: z.string(),
		executable: z.string(),
	})
	.readonly();

export type ProcessIdentity = z.infer<typeof processIdentitySchema>;
export const cuaReadbackSchema = z
	.strictObject({
		kind: z.literal('pty-dimension-readback').default('pty-dimension-readback'),
		surface: z.enum(['sessions', 'quota']),
		identity: processIdentitySchema,
		viewport: viewportSchema,
		source_sha256: z.string(),
		binary_sha256: z.string(),
		node_sha256: sha256Schema,
		observed_at: z.iso.datetime({ offset: true }),
	})
	.readonly();

export type CuaPtyReadback = z.infer<typeof cuaReadbackSchema>;
export const receiptImageSchema = z
	.strictObject({
		checkpoint: z.string().regex(/^compact-(sessions|quota)-[a-z]+(?:-[a-z]+)*$/u),
		png_sha256: sha256Schema,
	})
	.readonly();

export const filterReceiptSchema = z
	.strictObject({
		surface: z.enum(['sessions', 'quota']),
		columns: z.number().int(),
		rows: z.number().int(),
		binary_sha256: z.string(),
		node_sha256: sha256Schema,
		vhs_sha256: z.string(),
		ttyd_sha256: z.string(),
		verifier_sha256: z.string(),
		verifier_source_sha256: z.string(),
		verifier_checkpoint_source_sha256: sha256Schema,
		driver_source_sha256: z.string(),
		tape_driver_source_sha256: z.string(),
		fixed_child_sha256: z.string(),
		tape_sha256: z.string(),
		state_and_secrets_unchanged: z.literal(true),
		provider_requests: z.literal(0),
		child_exit: z.literal(0),
		owned_processes_stopped: z.literal(true),
		fixture: z.string(),
		images: z
			.array(receiptImageSchema)
			.min(6)
			.refine(
				(images) => new Set(images.map((image) => image.checkpoint)).size === images.length,
				'duplicate checkpoint in capture receipt',
			),
	})
	.readonly();

export const capturePlanSchema = z
	.strictObject({
		repository: absolutePathSchema,
		output: absolutePathSchema,
		temporary_directory: absolutePathSchema,
		source: sourceBindingSchema,
		node: artifactBindingSchema,
		capture_driver: artifactBindingSchema,
		harness_binary: artifactBindingSchema,
		vhs: artifactBindingSchema,
		ttyd: artifactBindingSchema,
		image_verifier: artifactBindingSchema,
		dynamic_library_path: z.string().min(1),
		build_declaration: z.string().min(1),
		cua_artifact_scenes: z
			.array(sceneNameSchema)
			.max(128)
			.default([])
			.refine(
				(scenes) => new Set(scenes).size === scenes.length,
				'duplicate declared original-artifact scene',
			),
	})
	.readonly();

export type CapturePlan = z.infer<typeof capturePlanSchema>;
export const componentPlanSchema = capturePlanSchema
	.unwrap()
	.extend({
		component_binary: artifactBindingSchema,
		grids: z
			.array(viewportSchema)
			.min(1)
			.max(6)
			.refine((grids) => {
				const allowed = new Set(['120x36', '76x30', '48x24', '48x12', '40x24', '32x24']);
				return (
					new Set(grids.map((grid) => `${grid.columns}x${grid.rows}`)).size === grids.length &&
					grids.every((grid) => allowed.has(`${grid.columns}x${grid.rows}`))
				);
			}, 'component grids must be unique supported viewports'),
		scenario: z.enum(['normal', 'visual-hard-cases']).default('normal'),
	})
	.readonly();

export type ComponentPlan = z.infer<typeof componentPlanSchema>;
export const componentReceiptSchema = z
	.strictObject({
		kind: z.enum(['compiled-quota-component-normal', 'compiled-quota-component-hard-cases']),
		columns: z.number().int(),
		rows: z.number().int(),
		binary_sha256: z.string(),
		node_sha256: sha256Schema,
		verifier_sha256: z.string(),
		checkpoint_source_sha256: z.string(),
		recognition_source_sha256: z.string(),
		driver_source_sha256: z.string(),
		tape_source_sha256: z.string(),
		fixed_child_sha256: z.string(),
		tape_sha256: z.string(),
		child_exit: z.literal(0),
		external_requests: z.literal(0),
		persisted_writes: z.literal(0),
		owned_children_stopped: z.literal(true),
		images: z.array(
			z
				.strictObject({
					checkpoint: z.string().regex(/^component-quota-[a-z]+(?:-[a-z]+)*$/u),
					png_sha256: sha256Schema,
				})
				.readonly(),
		),
	})
	.readonly();
