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
// Same shape as the verification runtime's identity records: ownership follows the real UID (ps
// `ruid`, Node `process.getuid()`); the effective UID (ps `uid`) is evidence only.
export const processIdentitySchema = z
	.strictObject({
		pid: z.number().int(),
		realUid: z.number().int(),
		effectiveUid: z.number().int(),
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
export const capturePlanSchema = z
	.strictObject({
		repository: z.string(),
		output: absolutePathSchema,
		temporary_directory: absolutePathSchema,
		source: sourceBindingSchema,
		node: artifactBindingSchema,
		capture_driver: artifactBindingSchema,
		harness_binary: artifactBindingSchema,
		build_declaration: z.string().min(1),
		cua_artifact_scenes: z.array(sceneNameSchema).default([]),
	})
	.readonly();
export type CapturePlan = z.infer<typeof capturePlanSchema>;
