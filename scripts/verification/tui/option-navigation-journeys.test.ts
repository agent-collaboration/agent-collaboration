/** Coverage admission, not product proof. The source decision supplies this closed context/key set. */
import { expect, test } from 'vitest';

import { s9GridSizes, s9JourneySchema } from './s9-journey-definition.ts';
import { optionNavigationJourneys } from './s9-journeys/option-navigation.ts';
import { validateS9Journeys } from './s9-prestage.ts';

test('all picker/quota list and Help aliases retain both modes, the consumed-key profile and independent expected destinations', () => {
	validateS9Journeys();
	expect(optionNavigationJourneys.map((journey) => journey.journeyId)).toEqual([
		'picker-list-option-navigation',
		'quota-list-option-navigation',
		'sessions-help-option-navigation',
		'quota-help-option-navigation',
	]);
	for (const candidate of optionNavigationJourneys) {
		const journey = s9JourneySchema.parse(candidate);
		expect(journey.inputProfile).toBe('example-ghostty-keybinds');
		expect(journey.keys).toEqual(['legacy', 'kitty']);
		expect(journey.grids).toEqual([...s9GridSizes]);
		expect(
			journey.steps
				.filter((step) => step.kind === 'press' && step.key.startsWith('Alt+'))
				.map((step) => (step.kind === 'press' ? step.key : '')),
		).toEqual(['Alt+Shift+Up', 'Alt+Down', 'Alt+Up', 'Alt+Shift+Down']);
		expect(journey.admissionInputs.join(' ')).toContain('exact Alt/Alt+Shift Up/Down dispatch ACK');
		expect(journey.fixtures.join(' ')).toContain('not derived from output');
		expect(journey.availability.state).toBe('absent-at-base');
	}
});
