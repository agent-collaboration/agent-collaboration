/** Draft journey proof ports are independently checked against the adopted source split. */
import { expect, test } from 'vitest';

import { authoredFlows, validateAuthoredFlows } from './flow-catalogue.ts';
import { newDialogJourneys } from './new-dialog.ts';

test('all New activation routes cover default, local and configured with the correct proof port', () => {
	validateAuthoredFlows();
	expect(newDialogJourneys).toHaveLength(9);
	for (const source of ['default', 'local', 'configured']) {
		const journeys = newDialogJourneys.filter((journey) =>
			journey.journeyId.endsWith('-' + source),
		);
		expect(journeys).toHaveLength(3);
		for (const journey of journeys)
			for (const step of journey.steps.filter((candidate) => candidate.kind === 'readback')) {
				expect(step.port).toBe(
					source === 'configured' ? 'preparation' : 'source-qualified-outcome',
				);
				expect(step.checks).toContain(
					source === 'configured' ? 'configured-start-once' : 'returned-new-once',
				);
			}
	}
});

test('U4 return drafts use same-ID HostedCodex service/endpoint controls, not provider Open', () => {
	const rows = authoredFlows.filter((journey) => journey.requirements.includes('U4/R4'));
	expect(rows).toHaveLength(2);
	for (const row of rows) {
		expect(row.fixtures.join(' ')).toContain('HostedCodex');
		expect(row.wrongOutcomes).toContain('same ID but wrong service');
		expect(row.wrongOutcomes).toContain('same ID but wrong endpoint');
	}
});

test('source setup selects the real New draft and LocalCodex keeps its legitimate single machine', () => {
	for (const journey of newDialogJourneys) {
		expect(journey.steps[0]).toEqual({ kind: 'press', key: 'Ctrl+N' });
		expect(journey.steps[1]?.kind).toBe('fixture');
		const dropdown = journey.steps.find((step) => step.kind === 'capture');
		if (dropdown?.kind !== 'capture') throw new Error('New dropdown checkpoint missing');
		expect(dropdown.checks).toContain(
			journey.journeyId.endsWith('-local')
				? 'single-local-machine'
				: 'dropdown-highlight-uncommitted',
		);
		expect(journey.wrongOutcomes).toContain('Start invoked twice');
	}
});
