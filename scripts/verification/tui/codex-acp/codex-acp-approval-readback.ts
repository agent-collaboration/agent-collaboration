/**
 * Agent-side reading for the O5 approval journey, in three hooks the run drives around the CLI:
 * before any input (the Session is in the declared non-auto mode, the scratch directory is fresh,
 * no approval is pending), while the CLI shows the prompt (exactly one new pending record, with
 * its offered options), and after the turn ends (the record's final state and whether the scratch
 * file exists). The reading and its reasons live in
 * verification/observations/providers/acp/codex-approval-outcome.ts.
 *
 * The record reads go through the ordinary MCP route (`approval_list`, `provider_session_inspect`),
 * never a lifecycle or decision call. The port seam keeps the logic testable without a Router;
 * `mcpApprovalPort` is exercised only in a declared live run (D4). Polling counts attempts and
 * calls the injected `pause`; nothing here reads or sleeps on the system clock.
 */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { containsJsonSubset } from '../../../../verification/json-comparison.ts';
import {
	answered,
	callTool,
	record,
} from '../../../../verification/observations/mcp-control/journeys/journey-steps.ts';
import type { JourneyContext } from '../../../../verification/observations/mcp-control/journeys/journey-steps.ts';
import type { SessionRef } from '../../../../verification/observations/mcp-control/observations/observation-models.ts';
import { StreamingMcpClient } from '../../../../verification/observations/mcp-control/transport/streaming-mcp-client.ts';
import {
	APPROVAL_OUTCOME_FILE,
	approvalOutcomeFile,
	approvalRecordsFrom,
} from '../../../../verification/observations/providers/acp/codex-approval-outcome.ts';
import type {
	ApprovalOutcomeFile,
	ApprovalRecord,
} from '../../../../verification/observations/providers/acp/codex-approval-outcome.ts';
import { catalogRecordSchema } from '../../../../verification/observations/shared/evidence-models.ts';
import { readScratchFile, requireFreshScratch } from './codex-acp-approval-scenes.ts';
import type { ApprovalJourneyDeclaration } from './codex-acp-approval-scenes.ts';
import { cliMcpEndpoint } from './codex-acp-mcp-readback.ts';
import { routerSession } from './codex-acp-target.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';

/** What the readback needs from the Router: this Session's mode and its approval records. */
export interface ApprovalPort {
	readonly currentMode: () => Promise<string>;
	/** Every approval record whose requester is this Session, pending or not, with its options. */
	readonly sessionApprovals: () => Promise<readonly ApprovalRecord[]>;
}

export interface ApprovalReadbackHooks {
	readonly beforeJourney: () => Promise<void>;
	readonly afterPromptShown: () => Promise<void>;
	/** Writes `approval-outcome.json`, then refuses any violation. Returns what it wrote. */
	readonly afterTurnEnded: () => Promise<ApprovalOutcomeFile>;
}

export function approvalReadback(inputs: {
	readonly port: ApprovalPort;
	readonly declaration: ApprovalJourneyDeclaration;
	readonly evidence: string;
	// Awaited between two polls; production waits, tests resolve at once.
	readonly pause: () => Promise<void>;
	// The stated bound on every wait for a record to appear or to leave `pendingClientDecision`:
	// polls are counted as deadlineMs / pollIntervalMs, and reaching the bound fails the run.
	readonly deadlineMs: number;
	readonly pollIntervalMs: number;
}): ApprovalReadbackHooks {
	const { port, declaration, evidence, pause, deadlineMs, pollIntervalMs } = inputs;
	if (deadlineMs <= 0 || pollIntervalMs <= 0 || pollIntervalMs > deadlineMs)
		throw new Error(
			'the approval read deadline and poll interval must be positive, interval within deadline',
		);
	const maxPolls = Math.ceil(deadlineMs / pollIntervalMs);
	let knownIds: ReadonlySet<string> | null = null;
	let pending: ApprovalRecord | null = null;

	// Polls by recursion, one read per attempt, until `find` answers or the poll bound is reached.
	async function pollFor<Found>(
		find: (records: readonly ApprovalRecord[]) => Found | undefined,
		what: string,
		attempt = 0,
	): Promise<Found> {
		if (attempt >= maxPolls)
			throw new Error(`${what} within the ${deadlineMs} ms read deadline (${maxPolls} polls)`);
		const found = find(await port.sessionApprovals());
		if (found !== undefined) return found;
		await pause();
		return pollFor(find, what, attempt + 1);
	}

	// The record's last reading once it leaves `pendingClientDecision`, or at the poll bound; a record
	// that never leaves it is judged as the pending record it is.
	async function finalReading(requestId: string, attempt = 0): Promise<ApprovalRecord> {
		const reading = (await port.sessionApprovals()).find((entry) => entry.requestId === requestId);
		if (reading === undefined) throw new Error('the approval record vanished from the list');
		// At the deadline the still-pending record is returned, and the outcome fails the run.
		if (reading.state !== 'pendingClientDecision' || attempt + 1 >= maxPolls) return reading;
		await pause();
		return finalReading(requestId, attempt + 1);
	}

	return {
		beforeJourney: async (): Promise<void> => {
			requireFreshScratch(declaration.scratch_directory);
			const mode = await port.currentMode();
			if (declaration.provider !== 'claude' || mode !== declaration.session_mode)
				throw new Error(`the Session is in mode ${mode}, not ${declaration.session_mode}`);
			const baseline = await port.sessionApprovals();
			if (baseline.some((entry) => entry.state === 'pendingClientDecision'))
				throw new Error('an approval was already pending before the journey');
			knownIds = new Set(baseline.map((entry) => entry.requestId));
		},
		afterPromptShown: async (): Promise<void> => {
			if (knownIds === null) throw new Error('the approval baseline is absent');
			const known = knownIds;
			pending = await pollFor((records) => {
				const fresh = records.filter(
					(entry) => !known.has(entry.requestId) && entry.state === 'pendingClientDecision',
				);
				if (fresh.length > 1) throw new Error('more than one new pending approval');
				return fresh[0];
			}, 'no new pending approval record appeared');
		},
		afterTurnEnded: async (): Promise<ApprovalOutcomeFile> => {
			if (pending === null) throw new Error('no pending approval was read while the prompt showed');
			const final = await finalReading(pending.requestId);
			const file = readScratchFile(declaration.scratch_directory, declaration.file_name);
			const answer = declaration.answer;
			const outcome = approvalOutcomeFile({
				case: answer.case,
				cliDecision: answer.case === 'timeout' ? null : answer.cli_decision,
				readings: { pending, final, filePresent: file === 'present' },
			});
			writeFileSync(join(evidence, APPROVAL_OUTCOME_FILE), JSON.stringify(outcome, null, 2), {
				mode: 0o600,
				flag: 'wx',
			});
			if (outcome.violations.length > 0)
				throw new Error(`approval outcome: ${outcome.violations.join('; ')}`);
			return outcome;
		},
	};
}

/** The MCP-backed port for a declared live run; never used by a unit test. */
export function mcpApprovalPort(inputs: {
	readonly target: CodexAcpTarget;
	readonly evidence: string;
	readonly deadlineMs: number;
}): ApprovalPort {
	const { target, evidence } = inputs;
	const endpoint = cliMcpEndpoint(target);
	const context: JourneyContext = {
		client: new StreamingMcpClient(endpoint),
		evidence,
		deadline: Date.now() + inputs.deadlineMs,
	};
	const session: SessionRef = {
		endpoint: { serviceId: target.expected_service_id, endpointId: target.endpoint_id },
		sessionId: routerSession(target).providerSessionId,
	};
	let initialized = false;
	const ready = async (): Promise<void> => {
		if (cliMcpEndpoint(target) !== endpoint) throw new Error('CLI MCP endpoint changed');
		if (initialized) return;
		const outcome = await context.client.initialize(15_000);
		record(context, { step: 'approval-mcp-initialize', outcome });
		answered(outcome, 'approval MCP initialize');
		initialized = true;
	};
	let step = 0;
	return {
		currentMode: async (): Promise<string> => {
			await ready();
			const outcome = await callTool(
				context,
				`approval-inspect-${step++}`,
				'provider_session_inspect',
				{ target: session },
				{ timeoutMs: 15_000 },
			);
			return catalogRecordSchema.parse(answered(outcome, 'approval session inspect'))
				.settingsCatalog.currentMode;
		},
		sessionApprovals: async (): Promise<readonly ApprovalRecord[]> => {
			await ready();
			const outcome = await callTool(
				context,
				`approval-list-${step++}`,
				'approval_list',
				{ pending: false, includeOptions: true },
				{ timeoutMs: 15_000 },
			);
			return approvalRecordsFrom(answered(outcome, 'approval list')).filter((entry) =>
				containsJsonSubset(entry.requester, session),
			);
		},
	};
}
