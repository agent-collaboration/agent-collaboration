/**
 * The `codex-acp` child launch behind the shared fixture-child wrapper. Immediately before spawn it
 * re-qualifies the declared installed-Codex target (binary hash and version, Host service/epoch,
 * endpoint socket, trusted cwd, raw Session to native alias, observer correlation) and returns that
 * target's exact argv with the inherited normal environment, so HOME and CODEX_HOME stay the
 * user's own. fixture-child.ts imports this module; this module imports fixture-child types only.
 */
import { codexAcpTargetSchema, qualifyCodexAcpTarget } from './codex-acp-target.ts';
import type { ChildLaunch, FixtureCommand } from './fixture-child.ts';

const CODEX_ACP_FIELDS: ReadonlySet<string> = new Set([
	'surface',
	'binary',
	'binary_sha256',
	'target',
	'output',
]);

export function codexChildLaunch(configuration: FixtureCommand): ChildLaunch {
	const fields = Object.keys(configuration);
	if (
		fields.length !== CODEX_ACP_FIELDS.size ||
		fields.some((field) => !CODEX_ACP_FIELDS.has(field))
	)
		throw new Error('codex-acp launch requires exact bound configuration fields');
	const target = codexAcpTargetSchema.parse(JSON.parse(configuration['target'] ?? 'null'));
	if (
		configuration['binary'] !== target.codex.path ||
		configuration['binary_sha256'] !== target.codex.sha256
	)
		throw new Error('codex-acp binary binding differs from its target');
	const launch = qualifyCodexAcpTarget(target);
	const environment: Record<string, string> = {};
	for (const [name, value] of Object.entries(process.env))
		if (value !== undefined) environment[name] = value;
	return {
		executable: launch.program,
		arguments: launch.arguments,
		environment,
		cwd: launch.cwd,
	};
}
