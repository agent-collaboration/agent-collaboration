/** F's corrected background outcome: start, attach, detach with the same job alive. */
import { z } from 'zod';
export const claudeBackgroundObservationSchema = z.strictObject({
	beforeJobIds: z.array(z.string().min(1)),
	afterStartJobIds: z.array(z.string().min(1)),
	returnedJobId: z.string().min(1),
	attachedJobId: z.string().min(1).nullable(),
	detachedJobId: z.string().min(1).nullable(),
	runningAfterDetachJobIds: z.array(z.string().min(1)),
	termiosBefore: z.string().min(1),
	termiosAfterDetach: z.string().min(1),
	terminalReleased: z.boolean(),
});
export function checkClaudeBackgroundObservation(value: unknown): readonly string[] {
	const observed = claudeBackgroundObservationSchema.parse(value);
	const failures: string[] = [];
	const before = new Set(observed.beforeJobIds);
	const after = new Set(observed.afterStartJobIds);
	if (
		before.size !== observed.beforeJobIds.length ||
		after.size !== observed.afterStartJobIds.length
	)
		failures.push('native job inventory contains duplicate identities');
	const created = [...after].filter((job) => !before.has(job));
	if (created.length !== 1 || created[0] !== observed.returnedJobId)
		failures.push('background New did not register exactly the newly owned returned job');
	if (observed.attachedJobId !== observed.returnedJobId)
		failures.push('background New returned without attaching to its job');
	if (observed.detachedJobId !== observed.returnedJobId || !observed.terminalReleased)
		failures.push('detach did not release the attached terminal');
	if (!observed.runningAfterDetachJobIds.includes(observed.returnedJobId))
		failures.push('detach did not leave the same job running');
	if (observed.termiosBefore !== observed.termiosAfterDetach)
		failures.push('detach did not restore termios');
	return failures;
}
