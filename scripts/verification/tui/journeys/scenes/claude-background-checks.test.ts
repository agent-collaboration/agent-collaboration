import { expect, test } from 'vitest';

import { checkClaudeBackgroundObservation } from './claude-background-checks.ts';
const good = {
	beforeJobIds: ['existing-job'],
	afterStartJobIds: ['existing-job', 'new-job'],
	returnedJobId: 'new-job',
	attachedJobId: 'new-job',
	detachedJobId: 'new-job',
	runningAfterDetachJobIds: ['existing-job', 'new-job'],
	termiosBefore: 'kernel-normal-snapshot',
	termiosAfterDetach: 'kernel-normal-snapshot',
	terminalReleased: true,
};
test('background New must attach and detach with one correlated job still running', () => {
	expect(checkClaudeBackgroundObservation(good)).toEqual([]);
	expect(
		checkClaudeBackgroundObservation({ ...good, attachedJobId: null, detachedJobId: null }),
	).toContain('background New returned without attaching to its job');
	expect(
		checkClaudeBackgroundObservation({
			...good,
			afterStartJobIds: [...good.afterStartJobIds, 'duplicate-new-job'],
		}),
	).not.toEqual([]);
	expect(checkClaudeBackgroundObservation({ ...good, attachedJobId: 'existing-job' })).not.toEqual(
		[],
	);
	expect(
		checkClaudeBackgroundObservation({ ...good, runningAfterDetachJobIds: ['existing-job'] }),
	).not.toEqual([]);
	expect(checkClaudeBackgroundObservation({ ...good, termiosAfterDetach: 'changed' })).not.toEqual(
		[],
	);
});
