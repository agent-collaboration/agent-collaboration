import { isMain, reportCliFailure, strictArguments, usageFailure } from './cli-arguments.ts';
import { coverageGaps } from './scene-contracts.ts';
import { sourceBinding } from './source-binding.ts';
export function evidenceReadbackMain(args: readonly string[]): void {
	const { values } = strictArguments(
		args,
		{ help: { type: 'boolean' }, source: { type: 'string' }, gaps: { type: 'boolean' } },
		false,
	);
	if (values.help) {
		process.stdout.write('evidence-readback.ts --source PATH | --gaps\n');
		return;
	}
	if ([values.source !== undefined, values.gaps].filter(Boolean).length !== 1)
		usageFailure('choose --source or --gaps');
	process.stdout.write(
		JSON.stringify(
			values.source !== undefined
				? sourceBinding(values.source)
				: { status: 'coverage-declared-separately', gaps: coverageGaps() },
			null,
			2,
		) + '\n',
	);
}
if (isMain(import.meta.url)) {
	try {
		evidenceReadbackMain(process.argv.slice(2));
	} catch (error) {
		reportCliFailure(error);
	}
}
