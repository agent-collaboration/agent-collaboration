import { writeFileSync } from 'node:fs';

import { isMain, reportCliFailure, strictArguments, usageFailure } from './cli-arguments.ts';
import { readCuaState, requireSamePty } from './cua-readback.ts';
import { capturePlanSchema, cuaReadbackSchema, viewportSchema } from './evidence-schema.ts';
import { coverageGaps } from './scene-contracts.ts';
import { isInside, readJson, sourceBinding } from './source-binding.ts';
export function evidenceReadbackMain(args: readonly string[]): void {
	const { values } = strictArguments(
		args,
		{
			help: { type: 'boolean' },
			source: { type: 'string' },
			gaps: { type: 'boolean' },
			'cua-plan': { type: 'string' },
			surface: { type: 'string' },
			columns: { type: 'string' },
			rows: { type: 'string' },
			record: { type: 'string' },
			baseline: { type: 'string' },
		},
		false,
	);
	if (values.help) {
		process.stdout.write(
			'evidence-readback.ts --source PATH | --gaps | --cua-plan PLAN --surface sessions|quota --columns N --rows N --record FILE [--baseline FILE]\n',
		);
		return;
	}
	if (
		[values.source !== undefined, values.gaps, values['cua-plan'] !== undefined].filter(Boolean)
			.length !== 1
	)
		usageFailure('choose exactly one of --source, --gaps or --cua-plan');
	const cuaFields = [values.surface, values.columns, values.rows, values.record, values.baseline];
	if (values['cua-plan'] === undefined && cuaFields.some((value) => value !== undefined))
		usageFailure('CUA options require --cua-plan');
	if (values['cua-plan'] !== undefined) {
		const surface = values.surface;
		const record = values.record;
		const grid = viewportSchema.safeParse({
			columns: Number(values.columns),
			rows: Number(values.rows),
		});
		if ((surface !== 'sessions' && surface !== 'quota') || record === undefined || !grid.success)
			usageFailure('CUA readback needs valid --surface, --columns, --rows and --record');
		const plan = capturePlanSchema.parse(readJson(values['cua-plan']));
		if (!isInside(plan.output, record))
			throw new Error('CUA readback record must be inside declared output');
		const state = readCuaState(plan, surface, grid.data);
		if (values.baseline !== undefined)
			requireSamePty(state, cuaReadbackSchema.parse(readJson(values.baseline)));
		writeFileSync(record, JSON.stringify(state, null, 2), { flag: 'wx' });
		process.stdout.write(`${JSON.stringify(state, null, 2)}\n`);
		return;
	}
	if (values.source !== undefined) {
		process.stdout.write(`${JSON.stringify(sourceBinding(values.source), null, 2)}\n`);
		return;
	}
	process.stdout.write(
		`${JSON.stringify({ status: 'draft-unobserved', gaps: coverageGaps() }, null, 2)}\n`,
	);
}
if (isMain(import.meta.url)) {
	try {
		evidenceReadbackMain(process.argv.slice(2));
	} catch (error) {
		reportCliFailure(error);
	}
}
