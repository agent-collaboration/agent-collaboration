import { expect, test } from 'vitest';

import { authoredFlows } from './flow-catalogue.ts';
import { pickerFlowAdmission } from './picker-flow-admission.ts';

test.each([
	['same-id-hosted-codex-fork', 'fork-ssh-profile'],
	['sessions-toolbar-and-menu', 'claude-new-success'],
	['picker-list-option-navigation', 'large-list'],
	['sessions-help-option-navigation', 'large-list'],
])('unimplemented fixture %s names its exact contract before launch', (name, contract) => {
	const journey = authoredFlows.find((flow) => flow.journeyId === name);
	if (journey === undefined) throw new Error('authored journey missing');
	const gaps = pickerFlowAdmission({ journey, grid: '120x36', keys: 'legacy' }, null);
	expect(gaps).toHaveLength(1);
	expect(gaps[0]?.kind).toBe('fixture');
	expect(gaps[0]?.name).toContain(contract);
});
