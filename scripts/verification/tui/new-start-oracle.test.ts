/** Draft journey proof ports are independently checked against the adopted source split. */
import { expect, test } from 'vitest';

import { newDialogJourneys } from './s9-journeys/new-dialog.ts';
import { s9AuthoredJourneys, validateS9Journeys } from './s9-prestage.ts';

test('all New activation routes cover default, local and configured with the correct proof port', () => {
	validateS9Journeys();
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
	const rows = s9AuthoredJourneys.filter((journey) => journey.requirements.includes('U4/R4'));
	expect(rows).toHaveLength(2);
	for (const row of rows) {
		expect(row.fixtures.join(' ')).toContain('HostedCodex');
		expect(row.wrongOutcomes).toContain('same ID but wrong service');
		expect(row.wrongOutcomes).toContain('same ID but wrong endpoint');
	}
});
