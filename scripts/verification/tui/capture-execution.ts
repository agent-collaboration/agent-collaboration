import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { collectImages } from './capture-collection.ts';
import {
	blankScene,
	captureArguments,
	cuaSurface,
	requireOneTest,
	selectionTest,
	staticCaptureBudgetSeconds,
	staticSurfaces,
} from './capture-selection.ts';
import type { CaptureSelection } from './capture-selection.ts';
import { collectComponentImages } from './component-collection.ts';
import {
	discoverCaptureTest,
	runCapturedCommand,
	startDriver,
	waitStaticCapture,
} from './driver-lifecycle.ts';
import { cuaLifecycleSchema, imageRejectionSchema } from './evidence-schema.ts';
import type { CapturePlan, ComponentPlan, FixtureScenario } from './evidence-schema.ts';
import { collectCuaArtifacts } from './original-artifacts.ts';
import {
	componentFixtureTest,
	componentWorkSeconds,
	coverageGaps,
	hardComponentCaptureTest,
	normalComponentCaptureTest,
	supportDirectory,
} from './scene-contracts.ts';
import { readJson, verifyArtifact, verifyTarget } from './source-binding.ts';
export function validateImageQualification(
	exitCode: number,
	output: string,
	selection: CaptureSelection = 'static-both',
): void {
	if (exitCode !== 1)
		throw new Error(
			'image oracle unavailable or blank accepted; use an authorized working execution context',
		);
	const rejection = imageRejectionSchema.parse(JSON.parse(output));
	const [checkpoint, marker] = blankScene(selection);
	if (rejection.checkpoint !== checkpoint || !rejection.missingMarkers.includes(marker))
		throw new Error('blank image did not fail its exact declared scene');
}
export async function checkDriver(
	plan: CapturePlan,
	selection: CaptureSelection = 'static-both',
	quotaFixture: FixtureScenario | null = null,
): Promise<void> {
	if (quotaFixture !== null && selection !== 'cua-quota')
		throw new Error('component fixture requires the quota CUA lease');
	verifyTarget(plan);
	requireOneTest(
		await discoverCaptureTest(
			[plan.capture_driver.path, selectionTest(selection), '--exact', '--list'],
			plan.repository,
		),
		selectionTest(selection),
	);
	if (quotaFixture !== null)
		requireOneTest(
			await discoverCaptureTest(
				[plan.harness_binary.path, componentFixtureTest, '--exact', '--ignored', '--list'],
				plan.repository,
			),
			componentFixtureTest,
		);
	const result = await runCapturedCommand({
		command: [
			plan.image_verifier.path,
			blankScene(selection)[0],
			join(plan.repository, supportDirectory, 'fixtures/blank-startup.png'),
		],
		repository: plan.repository,
		timeoutSeconds: 30,
	});
	try {
		validateImageQualification(result.exit, result.stdout, selection);
	} catch (error) {
		throw new Error(`${String(error)}; oracle stderr: ${result.stderr.trim()}`, { cause: error });
	}
}
export function captureEnvironment(plan: CapturePlan): NodeJS.ProcessEnv {
	const environment = { ...process.env };
	for (const key of [
		'QUOTA_STORYBOARD_CUA_SURFACE',
		'QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY',
		'QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO',
		'QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES',
	])
		delete environment[key];
	return {
		...environment,
		QUOTA_STORYBOARD_NODE: plan.node.path,
		QUOTA_STORYBOARD_NODE_SHA256: plan.node.sha256,
		QUOTA_STORYBOARD_OUTPUT: plan.output,
		QUOTA_STORYBOARD_VHS: plan.vhs.path,
		QUOTA_STORYBOARD_TTYD: plan.ttyd.path,
		QUOTA_STORYBOARD_IMAGE_VERIFIER: plan.image_verifier.path,
		QUOTA_STORYBOARD_DYLD_LIBRARY_PATH: plan.dynamic_library_path,
		TMPDIR: plan.temporary_directory,
	};
}
export async function executeDeclaredDriver(options: {
	readonly plan: CapturePlan;
	readonly command: readonly string[];
	readonly environment: NodeJS.ProcessEnv;
	readonly workSeconds: number;
	readonly cleanupSeconds: number;
	readonly stopFile?: string;
}): Promise<number> {
	writeFileSync(join(options.plan.output, 'command.json'), JSON.stringify(options.command));
	const out = openSync(join(options.plan.output, 'driver.stdout'), 'w');
	const err = openSync(join(options.plan.output, 'driver.stderr'), 'w');
	try {
		const child = startDriver(options.command, {
			cwd: options.plan.repository,
			env: options.environment,
			stdio: ['ignore', out, err],
		});
		return await waitStaticCapture(
			child,
			options.plan.output,
			{ work_seconds: options.workSeconds, cleanup_seconds: options.cleanupSeconds },
			options.stopFile,
		);
	} finally {
		closeSync(out);
		closeSync(err);
	}
}
export async function runCapture(
	plan: CapturePlan,
	selection: CaptureSelection = 'static-both',
	quotaFixture: FixtureScenario | null = null,
): Promise<void> {
	const surface = cuaSurface(selection);
	if (quotaFixture !== null && surface !== 'quota')
		throw new Error('component fixture requires the quota CUA lease');
	if (surface !== null && plan.cua_artifact_scenes.length === 0)
		throw new Error('CUA lease requires exact predeclared original-artifact scenes');
	await checkDriver(plan, selection, quotaFixture);
	mkdirSync(plan.output, { mode: 0o700 });
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	writeFileSync(
		join(plan.output, 'cua-request.json'),
		JSON.stringify({ surface, quota_fixture_scenario: quotaFixture }),
	);
	const environment = captureEnvironment(plan);
	if (surface !== null) {
		environment['QUOTA_STORYBOARD_CUA_SURFACE'] = surface;
		environment['QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES'] = plan.cua_artifact_scenes.join(',');
	}
	if (quotaFixture !== null) {
		environment['QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY'] = plan.harness_binary.path;
		environment['QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO'] = quotaFixture;
	}
	const exit = await executeDeclaredDriver({
		plan,
		command: captureArguments(plan.capture_driver.path, selection),
		environment,
		workSeconds: surface === null ? staticCaptureBudgetSeconds(selection) - 90 : 660,
		cleanupSeconds: 90,
		...(surface === null ? {} : { stopFile: join(plan.output, `cua-${surface}`, 'cua-stop') }),
	});
	verifyTarget(plan);
	if (
		exit !== 0 ||
		!readFileSync(join(plan.output, 'driver.stdout'), 'utf8').includes(
			'test result: ok. 1 passed; 0 failed; 0 ignored;',
		)
	)
		throw new Error('capture did not run exactly one passing non-ignored test; retain logs');
	if (surface !== null) {
		const receipt = cuaLifecycleSchema.parse(
			readJson(join(plan.output, `cua-${surface}`, 'cua-receipt.json')),
		);
		if (
			receipt.node_sha256 !== plan.node.sha256 ||
			receipt.surface !== surface ||
			receipt.binary_sha256 !== plan.harness_binary.sha256 ||
			receipt.quota_fixture_scenario !== quotaFixture
		)
			throw new Error('CUA lifecycle receipt target mismatch');
		writeFileSync(
			join(plan.output, 'cua-lifecycle-summary.json'),
			JSON.stringify(
				{
					target: plan,
					receipt,
					original_artifacts: collectCuaArtifacts(plan, surface, receipt.artifact_count),
					complete: false,
					limits:
						'Lifecycle only; same-PTY readbacks and original CUA screenshots must be checked separately.',
				},
				null,
				2,
			),
		);
		return;
	}
	const images = collectImages(plan, selection);
	writeFileSync(
		join(plan.output, 'gallery-manifest.json'),
		JSON.stringify(
			{
				kind: 'original-compiled-fixture-pty',
				created_at: new Date().toISOString(),
				target: plan,
				capture_selection: selection,
				capture_test: selectionTest(selection),
				captured_surfaces: staticSurfaces(selection),
				images,
				gaps: coverageGaps(),
				complete: false,
			},
			null,
			2,
		),
	);
}
export async function runComponentCapture(plan: ComponentPlan): Promise<void> {
	verifyTarget(plan);
	verifyArtifact(plan.component_binary);
	const selector =
		plan.scenario === 'normal' ? normalComponentCaptureTest : hardComponentCaptureTest;
	await Promise.all(
		[
			[plan.capture_driver.path, selector],
			[plan.component_binary.path, componentFixtureTest],
		].map(async ([binary, name]) => {
			if (binary === undefined || name === undefined)
				throw new Error('incomplete component selector');
			requireOneTest(
				await discoverCaptureTest([binary, name, '--exact', '--list'], plan.repository),
				name,
			);
		}),
	);
	const blankCheckpoint =
		plan.scenario === 'normal'
			? 'component-quota-policy-saved'
			: 'component-quota-hard-beta-windows';
	const blank = await runCapturedCommand({
		command: [
			plan.image_verifier.path,
			blankCheckpoint,
			join(plan.repository, supportDirectory, 'fixtures/blank-startup.png'),
		],
		repository: plan.repository,
		timeoutSeconds: 30,
	});
	if (blank.exit !== 1) throw new Error('component blank oracle unavailable or accepted');
	if (imageRejectionSchema.parse(JSON.parse(blank.stdout)).checkpoint !== blankCheckpoint)
		throw new Error('component blank oracle wrong checkpoint');
	mkdirSync(plan.output, { mode: 0o700 });
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	const environment = captureEnvironment(plan);
	environment['QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY'] = plan.component_binary.path;
	environment['QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO'] = plan.scenario;
	environment['QUOTA_STORYBOARD_COMPONENT_VIEWPORTS'] = plan.grids
		.map((grid) => `${grid.columns}x${grid.rows}`)
		.join(',');
	const result = await executeDeclaredDriver({
		plan,
		command: [
			plan.capture_driver.path,
			selector,
			'--exact',
			'--ignored',
			'--nocapture',
			'--test-threads=1',
		],
		environment,
		workSeconds: componentWorkSeconds(plan),
		cleanupSeconds: 90,
	});
	if (result !== 0)
		throw new Error('component driver failed; retain originals and settle owned resources');
	verifyTarget(plan);
	verifyArtifact(plan.component_binary);
	const records = collectComponentImages(plan);
	writeFileSync(
		join(plan.output, 'component-gallery-manifest.json'),
		JSON.stringify(
			{
				kind: 'partial-compiled-component-proof',
				complete: false,
				images: records,
				pending: [
					'personal original image inspection',
					'pending scenarios/resize',
					'public/provider observations',
				],
			},
			null,
			2,
		),
	);
}
