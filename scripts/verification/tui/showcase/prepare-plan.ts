/** Declare source and executable identities before starting any showcase child. */
import { realpathSync } from 'node:fs';

import { isMain, reportCliFailure, strictArguments } from '../cli-arguments.ts';
import { gitOutput } from '../git-source.ts';
import { captureArtifact, sourceBinding } from '../source-binding.ts';
import { showcasePlanSchema } from './contracts.ts';

export function prepareShowcasePlan(args: readonly string[]): void {
	const { values } = strictArguments(
		args,
		{
			help: { type: 'boolean' },
			driver: { type: 'string' },
			example: { type: 'string' },
			output: { type: 'string' },
			chrome: { type: 'string' },
			profile: { type: 'string' },
			cases: { type: 'string' },
			variants: { type: 'string' },
			grids: { type: 'string' },
			keys: { type: 'string' },
		},
		false,
	);
	if (values.help) {
		process.stdout.write(
			'showcase/prepare-plan.ts --driver ABSOLUTE_SIGNED_DRIVER --example ABSOLUTE_SIGNED_EXAMPLE --output FRESH_ABSOLUTE_DIRECTORY --chrome CHROME [--profile terminal-only|example-ghostty-keybinds] [--cases SC01,SC02] [--variants wide-dropdown-folder] [--grids 120x36,48x24] [--keys legacy,kitty]\nPrints a pinned plan; default coverage is all 27 scenarios, six adopted grids, both protocols, including four independent Quit surfaces.\n',
		);
		return;
	}
	if (process.version !== 'v26.12.1') throw new Error('showcase requires nominated Node v26.12.1');
	const repository = realpathSync(process.cwd());
	if (gitOutput(repository, ['rev-parse', '--show-toplevel']).trim() !== repository)
		throw new Error('prepare plan from repository root');
	if (
		values.driver === undefined ||
		values.example === undefined ||
		values.output === undefined ||
		values.chrome === undefined
	)
		throw new Error('driver, example, output and Chrome are required');
	const grids = values.grids?.split(',').map((value) => {
		const [columns, rows, ...rest] = value.split('x');
		if (rest.length !== 0 || rows === undefined) throw new Error('grid must be COLUMNSxROWS');
		return { columns: Number(columns), rows: Number(rows) };
	});
	const plan = showcasePlanSchema.parse({
		kind: 'component-showcase',
		repository,
		source: sourceBinding(repository),
		driver: captureArtifact(values.driver),
		example: captureArtifact(values.example),
		node: captureArtifact(process.execPath),
		output: values.output,
		chrome: realpathSync(values.chrome),
		inputProfile: values.profile ?? 'terminal-only',
		...(grids === undefined ? {} : { grids }),
		...(values.cases === undefined ? {} : { cases: values.cases.split(',') }),
		...(values.variants === undefined ? {} : { variantLabels: values.variants.split(',') }),
		...(values.keys === undefined ? {} : { keys: values.keys.split(',') }),
	});
	process.stdout.write(JSON.stringify(plan, null, 2) + '\n');
}
if (isMain(import.meta.url))
	try {
		prepareShowcasePlan(process.argv.slice(2));
	} catch (error) {
		reportCliFailure(error);
	}
