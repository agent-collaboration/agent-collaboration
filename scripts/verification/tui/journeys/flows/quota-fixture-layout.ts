/** Private per-invocation files for the existing declared quota wrapper; never launches a child. */
import { createHash } from 'node:crypto';
import {
	copyFileSync,
	lstatSync,
	mkdirSync,
	readdirSync,
	realpathSync,
	symlinkSync,
	writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

import type { ArtifactBinding } from '../../evidence/evidence-schema.ts';
import { verifyArtifact } from '../../evidence/source-binding.ts';
import { quotaEffectLaunch } from '../../fixtures/quota-effect-launch.ts';
import { shortTemporaryRoot } from '../../fixtures/short-temporary-root.ts';
import {
	quotaEffectConfigurationSchema,
	quotaRequiresPrivateGuard,
} from './quota-fixture-contract.ts';
import type { QuotaEffectConfiguration, QuotaEffectScenario } from './quota-fixture-contract.ts';

const jsonBytes = (value: unknown): string => JSON.stringify(value, null, 2) + '\n';
const digest = (value: string): string => createHash('sha256').update(value).digest('hex');
export interface QuotaFixtureLayout {
	readonly product: ArtifactBinding;
	readonly output: string;
	readonly configurationPath: string;
	readonly configuration: QuotaEffectConfiguration;
	readonly configurationBytes: string;
	readonly commandPath: string;
	readonly command: Readonly<Record<string, string>>;
	readonly commandBytes: string;
	readonly environment: Readonly<Record<string, string>>;
	readonly standin: ArtifactBinding | undefined;
}

/** Deterministic paths let admission inspect exactly the bindings later staged for execution. */
export function quotaFixtureLayout(options: {
	readonly scenario: QuotaEffectScenario;
	readonly product: ArtifactBinding;
	readonly standin?: ArtifactBinding;
	readonly repository: string;
	readonly evidence: string;
}): QuotaFixtureLayout {
	const output = join(
		shortTemporaryRoot(),
		'2026-11-qb-' + digest(options.evidence + '\n' + options.scenario).slice(0, 16),
	);
	const guarded = quotaRequiresPrivateGuard(options.scenario);
	if (guarded && options.standin === undefined)
		throw new Error('quota guarded fixture requires its declared stand-in');
	const configuration = quotaEffectConfigurationSchema.parse({
		scenario: options.scenario,
		outputDirectory: output,
		events: join(output, 'events.jsonl'),
		receipt: join(output, 'receipt.json'),
		controlSocket: join(output, 'control.sock'),
		dispatchReceipt: join(output, 'dispatch.jsonl'),
		...(guarded
			? {
					loginReceipt: join(output, 'login.jsonl'),
					loginGateSocket: join(output, 'login.sock'),
					loginStandin: options.standin?.path,
				}
			: {}),
	});
	const configurationPath = join(output, 'effect.json');
	const configurationBytes = jsonBytes(configuration);
	const command = {
		surface: 'quota-effects',
		binary: options.product.path,
		binary_sha256: options.product.sha256,
		approved_cwd: options.repository,
		output,
		effect_configuration: configurationPath,
		effect_configuration_sha256: digest(configurationBytes),
		...(guarded
			? {
					login_standin_sha256: options.standin?.sha256 ?? '',
					login_path_prefix: join(output, 'bin'),
				}
			: {}),
	};
	const commandBytes = jsonBytes(command),
		commandPath = join(output, 'fixture-command.json');
	return {
		product: options.product,
		output,
		configurationPath,
		configuration,
		configurationBytes,
		commandPath,
		command,
		commandBytes,
		environment: {
			TUI_QUOTA_EFFECT_OUTPUT: output,
			TUI_QUOTA_EFFECT_CONFIGURATION: commandPath,
			TUI_QUOTA_EFFECT_CONFIGURATION_SHA256: digest(commandBytes),
		},
		standin: guarded ? options.standin : undefined,
	};
}

/** Run only after whole-journey admission, with a fresh owned evidence directory. */
export function stageQuotaFixture(layout: QuotaFixtureLayout, evidence: string): void {
	if (realpathSync(evidence) !== evidence || !lstatSync(evidence).isDirectory())
		throw new Error('quota evidence must be canonical');
	verifyArtifact(layout.product);
	if (layout.standin !== undefined) verifyArtifact(layout.standin);
	mkdirSync(layout.output, { mode: 0o700 });
	if (layout.standin !== undefined) {
		verifyArtifact(layout.standin);
		const prefix = join(layout.output, 'bin');
		mkdirSync(prefix, { mode: 0o700 });
		symlinkSync(layout.standin.path, join(prefix, 'codex-router'));
	}
	writeFileSync(layout.configurationPath, layout.configurationBytes, { flag: 'wx', mode: 0o600 });
	writeFileSync(layout.commandPath, layout.commandBytes, { flag: 'wx', mode: 0o600 });
	quotaEffectLaunch(layout.command);
	writeFileSync(join(evidence, 'quota-fixture-layout.json'), jsonBytes(layout), {
		flag: 'wx',
		mode: 0o600,
	});
}

/** Call after the driver and product settle; preserve raw producer files, never copy live sockets. */
export function retainQuotaFixture(layout: QuotaFixtureLayout, evidence: string): void {
	const destination = join(evidence, 'quota-fixture');
	mkdirSync(destination, { mode: 0o700 });
	for (const name of readdirSync(layout.output)) {
		const path = join(layout.output, name);
		if (lstatSync(path).isFile()) copyFileSync(path, join(destination, name));
	}
}
