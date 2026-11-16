/**
 * O5 provider-usage readback for a Router target: after the comparison journey, reads the Session's
 * turns back through the same ordinary MCP route the prompt journey uses (cursor-chained
 * `events_observe`, never a lifecycle call) and retains each turn's Usage item payload as
 * `provider-usage.json`, the provider's own `used` and `size` that N6 stored unchanged.
 *
 * Exercised only against a live Router in a declared run. The pure reduction it ends with is tested
 * in verification/observations/providers/acp/codex-compare-evaluation.test.ts. A Usage item
 * published after its turn ended is outside every turn here and is not read.
 */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { encode } from '../../../../verification/exact-json.ts';
import {
	callTool,
	nextCursor,
	observe,
	observeReplayBaseline,
	record,
	answered,
} from '../../../../verification/observations/mcp-control/journeys/journey-steps.ts';
import type { JourneyContext } from '../../../../verification/observations/mcp-control/journeys/journey-steps.ts';
import { requireSelectedModel } from '../../../../verification/observations/mcp-control/observations/declared-model.ts';
import type { DeclaredProviderSelection } from '../../../../verification/observations/mcp-control/observations/declared-model.ts';
import { foldObservationReads } from '../../../../verification/observations/mcp-control/observations/observation-ledger.ts';
import type { ObservationRead } from '../../../../verification/observations/mcp-control/observations/observation-ledger.ts';
import type { SessionRef } from '../../../../verification/observations/mcp-control/observations/observation-models.ts';
import { reconstructTurns } from '../../../../verification/observations/mcp-control/observations/observation-turns.ts';
import { StreamingMcpClient } from '../../../../verification/observations/mcp-control/transport/streaming-mcp-client.ts';
import {
	PROVIDER_USAGE_FILE,
	providerUsageFromRouterTurns,
} from '../../../../verification/observations/providers/acp/codex-compare-evaluation.ts';
import { cliMcpEndpoint } from './codex-acp-mcp-readback.ts';
import { routerSession } from './codex-acp-target.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';

const MAX_READS = 40;
const READ_TIMEOUT_SECONDS = 1;

export interface CompareUsageReadback {
	readonly beforeJourney: () => Promise<void>;
	/**
	 * Before the first Enter: the Session's selected model and options still equal the declared
	 * selection. Codex resume can apply settings to the Session (native_provider_settings), so the
	 * model proven before launch is checked again at the last moment before any model input.
	 */
	readonly beforeFirstSubmit: () => Promise<void>;
	readonly afterJourney: () => Promise<void>;
}

/** The baseline is read before the CLI acts; the turns after it are read once the CLI has quit. */
export function compareUsageReadback(inputs: {
	readonly target: CodexAcpTarget;
	readonly evidence: string;
	readonly deadlineMs: number;
	readonly selection: DeclaredProviderSelection;
}): CompareUsageReadback {
	const { target, evidence, selection } = inputs;
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
	const reads: ObservationRead[] = [];
	let baselineReads = 0;
	const binding = (): void => {
		if (cliMcpEndpoint(target) !== endpoint) throw new Error('CLI MCP endpoint changed');
	};
	// Reads continue by cursor until a read is not cut short by its result limit.
	async function readUntilComplete(attempt: number): Promise<void> {
		if (attempt >= MAX_READS) return;
		binding();
		reads.push(
			await observe(
				context,
				`compare-usage-${attempt}`,
				session,
				nextCursor(session, reads, baselineReads),
				READ_TIMEOUT_SECONDS,
			),
		);
		if (reads.at(-1)?.result.endReason === 'resultLimitReached')
			return readUntilComplete(attempt + 1);
	}
	return {
		beforeJourney: async (): Promise<void> => {
			binding();
			const initialized = await context.client.initialize(15_000);
			record(context, { step: 'compare-mcp-initialize', outcome: initialized });
			answered(initialized, 'compare MCP initialize');
			const baseline = await observeReplayBaseline(context, 'compare-baseline', session);
			binding();
			if (baseline === null) throw new Error('compare MCP baseline exceeded its read bound');
			reads.push(...baseline);
			baselineReads = reads.length;
			const ledger = foldObservationReads(session, reads, baselineReads);
			if (
				ledger.kind !== 'continuous' ||
				ledger.backendDisconnected ||
				ledger.baselineTooLarge.length > 0
			)
				throw new Error('compare MCP baseline is incomplete');
		},
		beforeFirstSubmit: async (): Promise<void> => {
			if (baselineReads === 0) throw new Error('compare MCP baseline is absent');
			binding();
			const outcome = await callTool(
				context,
				'compare-before-enter-model',
				'provider_session_inspect',
				{ target: session },
				{ timeoutMs: 15_000 },
			);
			binding();
			requireSelectedModel(answered(outcome, 'compare-before-enter-model'), session, selection);
		},
		afterJourney: async (): Promise<void> => {
			if (baselineReads === 0) throw new Error('compare MCP baseline is absent');
			await readUntilComplete(0);
			if (reads.at(-1)?.result.endReason === 'resultLimitReached')
				throw new Error('compare MCP history is incomplete at its read bound');
			const ledger = foldObservationReads(session, reads, baselineReads);
			if (ledger.kind !== 'continuous' || ledger.backendDisconnected || ledger.tooLarge.length > 0)
				throw new Error('compare MCP history is incomplete');
			const reconstruction = reconstructTurns(ledger.events);
			if (reconstruction.kind === 'rejected') throw new Error(reconstruction.reason);
			writeFileSync(
				join(evidence, PROVIDER_USAGE_FILE),
				encode(providerUsageFromRouterTurns(reconstruction.turns)),
				{ mode: 0o600, flag: 'wx' },
			);
		},
	};
}
