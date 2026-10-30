import { spawnSync } from 'node:child_process';
/** Public Sessions uses the same grid/image sink; Host/job proof remains separate. */
import { readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { cuaReadbackSchema } from './evidence-schema.ts';
import type { CuaPtyReadback, TerminalViewport } from './evidence-schema.ts';
import { encodeJourneyKey } from './journey-key.ts';
import { lookupProcessIdentity, sameIdentity } from './process-identity.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { PublicSessionsPlan } from './public-sessions.ts';
import { fileDigest, readJson, sourceDigest, verifyTarget } from './source-binding.ts';
import { gridLines } from './terminal-grid.ts';
import { renderOriginal } from './terminal-original.ts';
export const publicGridActionSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('key'), key: z.string() }),
	z.strictObject({ kind: z.literal('text'), text: z.string().min(1) }),
	z.strictObject({
		kind: z.literal('resize'),
		columns: z.number().int().positive(),
		rows: z.number().int().positive(),
	}),
	z.strictObject({
		kind: z.literal('checkpoint'),
		scene: z.string().regex(/^[a-z0-9-]+$/u),
		pattern: z.string().min(1),
	}),
]);
export function requireSamePty(after: CuaPtyReadback, before: CuaPtyReadback): void {
	if (
		after.node_sha256 !== before.node_sha256 ||
		after.surface !== before.surface ||
		after.source_sha256 !== before.source_sha256 ||
		after.binary_sha256 !== before.binary_sha256 ||
		!sameIdentity(after.identity, before.identity)
	)
		throw new Error('resize replaced the declared PTY or target');
}
export function readPublicGridState(
	plan: PublicSessionsPlan,
	expected: TerminalViewport,
): CuaPtyReadback {
	verifyTarget(plan);
	const output = join(plan.output, 'cua-sessions');
	const wrapperPid = Number(readFileSync(join(output, 'quota-wrapper.pid'), 'utf8').trim());
	const wrapper = lookupProcessIdentity(wrapperPid);
	// Same LC_ALL=C as the receipt writer, so the raw `lstart` bytes compare across locales.
	const identity = spawnSync('/bin/ps', ['-p', String(wrapperPid), '-o', 'uid=,lstart=,command='], {
		encoding: 'utf8',
		timeout: 3000,
		env: { ...process.env, LC_ALL: 'C' },
	});
	if (
		wrapper === null ||
		wrapper.realUid !== process.getuid?.() ||
		realpathSync(wrapper.executable) !== plan.node.path ||
		identity.status !== 0 ||
		identity.stdout !== readFileSync(join(output, 'quota-wrapper.identity'), 'utf8') ||
		!identity.stdout.includes(join(output, 'fixture-command.json'))
	)
		throw new Error('public wrapper process binding changed');
	const pid = Number(readFileSync(join(output, 'quota-child.pid'), 'utf8').trim());
	if (!Number.isInteger(pid) || pid <= 0) throw new Error('incomplete public child PID');
	const child = lookupProcessIdentity(pid);
	if (
		child === null ||
		child.realUid !== process.getuid?.() ||
		realpathSync(child.executable) !== plan.public_target.binary.path
	)
		throw new Error('public PTY identity does not match executable/owner');
	const values = readFileSync(join(output, 'quota-child.viewport'), 'utf8').trim().split(/\s+/u);
	if (
		values.length !== 2 ||
		Number(values[0]) !== expected.columns ||
		Number(values[1]) !== expected.rows
	)
		throw new Error('actual public PTY viewport differs from expected');
	return cuaReadbackSchema.parse({
		node_sha256: plan.node.sha256,
		surface: 'sessions',
		identity: child,
		viewport: expected,
		source_sha256: sourceDigest(plan.source),
		binary_sha256: plan.public_target.binary.sha256,
		observed_at: new Date().toISOString(),
	});
}
export async function runPublicGridCapture(
	plan: PublicSessionsPlan,
	environment: NodeJS.ProcessEnv,
): Promise<void> {
	const output = join(plan.output, 'cua-sessions');
	const driver = await PtyGridDriver.start({
		repository: plan.repository,
		driver: plan.capture_driver.path,
		surface: 'public-sessions',
		keys: plan.keys,
		evidence: output,
		publicEnvironment: {
			...environment,
			TUI_GRID_PUBLIC_ARTIFACT_COUNT: String(plan.cua_artifact_scenes.length),
		},
	});
	let viewport = { columns: 120, rows: 36 };
	let identity: CuaPtyReadback | null = null;
	const originals: unknown[] = [];
	let after = -1;
	try {
		await plan.actions.reduce<Promise<void>>(async (previous, action) => {
			await previous;
			if (action.kind === 'key' || action.kind === 'text') {
				after = driver.frameGeneration();
				await driver.send(
					action.kind === 'key'
						? encodeJourneyKey(action.key, plan.keys)
						: Buffer.from(action.text),
				);
			} else if (action.kind === 'resize') {
				after = driver.frameGeneration();
				viewport = { columns: action.columns, rows: action.rows };
				await driver.resize(viewport.columns, viewport.rows);
			} else {
				const expression = new RegExp(action.pattern, 'su');
				const grid = await driver.waitForGrid(
					action.scene,
					(candidate) => expression.test(gridLines(candidate).join('\n')),
					after,
				);
				const observed = readPublicGridState(plan, viewport);
				if (identity !== null) requireSamePty(observed, identity);
				identity = observed;
				writeFileSync(join(output, action.scene + '.grid.json'), JSON.stringify(grid, null, 2));
				writeFileSync(join(output, action.scene + '.pty.json'), JSON.stringify(observed, null, 2));
				originals.push(
					await renderOriginal({
						chrome: plan.chrome.path,
						output,
						bytes: driver.bytes(),
						grid,
						resizes: driver.resizeHistory(),
						columns: viewport.columns,
						rows: viewport.rows,
						checkpoint: action.scene,
						sourceHead: plan.source.head,
						binarySha256: plan.public_target.binary.sha256,
					}),
				);
			}
		}, Promise.resolve());
		const drain = async (): Promise<void> => {
			if ((await driver.receive()) !== 'eof') await drain();
		};
		await drain();
		if ((await driver.finish()) !== 0) throw new Error('public grid driver failed');
		verifyTarget(plan);
		writeFileSync(
			join(output, 'original-manifest.json'),
			JSON.stringify({ originals, physicalTerminal: false }, null, 2),
		);
		// This source-bound sink never claims the separate Host/provider job is settled.
		const receipt = readJson(join(output, 'public-cua-receipt.json'));
		const parsed = z
			.object({ public_capture_source_sha256: z.string(), fixed_child_sha256: z.string() })
			.parse(receipt);
		if (
			parsed.public_capture_source_sha256 !==
				fileDigest(
					join(
						plan.repository,
						'crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/public_sessions_capture.rs',
					),
				) ||
			parsed.fixed_child_sha256 !==
				fileDigest(join(plan.repository, 'scripts/verification/tui/fixture-child.ts'))
		)
			throw new Error('public capture source binding mismatch');
	} finally {
		await driver.stop();
	}
}
